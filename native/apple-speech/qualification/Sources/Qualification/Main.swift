import CoreML
import CryptoKit
import Darwin
import FluidAudio
import Foundation

struct Fixture: Decodable {
    let id: String
    let pcm: String
    let expected: [String]
    let reference: String
}
struct Experiment: Decodable {
    let terms: [String]
    let fixtures: [Fixture]
}
struct AssetManifest: Decodable {
    struct Asset: Decodable { let path: String; let bytes: Int; let sha256: String }
    let repo: String
    let revision: String
    let files: [Asset]
}
enum Failure: Error { case invalid(String) }
func seconds(_ start: ContinuousClock.Instant) -> Double {
    let parts = start.duration(to: .now).components
    return Double(parts.seconds) + Double(parts.attoseconds) / 1e18
}
func emit(_ value: [String: Any]) throws {
    let bytes = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    FileHandle.standardOutput.write(bytes); FileHandle.standardOutput.write(Data([10]))
}
func peakRSS() -> Int64 {
    var usage = rusage(); getrusage(RUSAGE_SELF, &usage)
    return Int64(usage.ru_maxrss)
}
func verify(_ directory: URL, manifest: URL) throws {
    let files = try JSONDecoder().decode(AssetManifest.self, from: Data(contentsOf: manifest)).files
    guard !files.isEmpty, files.count < 100 else { throw Failure.invalid("Invalid explicit asset manifest") }
    for entry in files {
        guard !entry.path.hasPrefix("/"), !entry.path.split(separator: "/").contains(".."), entry.bytes < 700_000_000 else {throw Failure.invalid("Invalid asset path or size")}
        let path = directory.appendingPathComponent(entry.path)
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular, attributes[.size] as? Int == entry.bytes else {throw Failure.invalid("Asset size/type mismatch: \(entry.path)")}
        let file = try FileHandle(forReadingFrom: path); defer {try? file.close()}
        var hash = SHA256()
        while let bytes = try file.read(upToCount: 1_048_576), !bytes.isEmpty {hash.update(data: bytes)}
        guard hash.finalize().map({String(format:"%02x",$0)}).joined() == entry.sha256 else {throw Failure.invalid("Asset checksum mismatch: \(entry.path)")}
    }
}
@main struct Qualification {
    static func main() async throws {
        let args = Array(CommandLine.arguments.dropFirst())
        guard args.count == 5 else {throw Failure.invalid("Usage: qualify-vocabulary TDT_DIR CTC_DIR CTC_MANIFEST TDT_MANIFEST EXPERIMENT_JSON")}
        let tdt = URL(fileURLWithPath: args[0], isDirectory: true)
        let ctc = URL(fileURLWithPath: args[1], isDirectory: true)
        try verify(ctc, manifest: URL(fileURLWithPath: args[2])); try verify(tdt, manifest: URL(fileURLWithPath: args[3]))
        let data = try Data(contentsOf: URL(fileURLWithPath: args[4])); guard data.count <= 65_536 else {throw Failure.invalid("Experiment metadata too large")}
        let experiment = try JSONDecoder().decode(Experiment.self, from: data)
        guard (1...64).contains(experiment.terms.count), experiment.terms.allSatisfy({(3...80).contains($0.count)}), experiment.fixtures.count <= 32 else {throw Failure.invalid("Experiment bounds exceeded")}
        let start = ContinuousClock.now
        let config = AsrModels.defaultConfiguration(); config.computeUnits = .cpuAndNeuralEngine
        let models = try AsrModels.loadLocal(from: tdt, version: .v3, configuration: config, encoderPrecision: .int8V2, encoderComputeUnits: .cpuAndNeuralEngine)
        let manager = AsrManager(config: ASRConfig(parallelChunkConcurrency: 1)); try await manager.loadModels(models)
        try emit(["event":"tdt_loaded","seconds":seconds(start),"peakRSSBytes":peakRSS()])
        var baselines: [(Fixture,[Float],ASRResult)] = []
        for fixture in experiment.fixtures {
            let path = URL(fileURLWithPath: fixture.pcm)
            let metadata = try FileManager.default.attributesOfItem(atPath: path.path)
            guard let size = metadata[.size] as? Int, size > 0, size <= 240_000 * 4, size % 4 == 0 else {throw Failure.invalid("Fixture must be <=15s mono16k f32")}
            let pcm = try Data(contentsOf:path)
            let samples = pcm.withUnsafeBytes {b in (0..<pcm.count/4).map {Float(bitPattern:UInt32(littleEndian:b.loadUnaligned(fromByteOffset:$0*4,as:UInt32.self)))}}
            guard samples.allSatisfy({$0.isFinite && abs($0)<=1}) else {throw Failure.invalid("Invalid fixture samples")}
            var state = try TdtDecoderState(); let clock = ContinuousClock.now
            let result = try await manager.transcribe(samples,decoderState:&state)
            baselines.append((fixture,samples,result))
            try emit(["event":"baseline","id":fixture.id,"text":result.text,"expected":fixture.expected,"reference":fixture.reference,"duration":Double(samples.count)/16_000,"seconds":seconds(clock),"peakRSSBytes":peakRSS()])
        }
        let ctcStart = ContinuousClock.now
        let ctcModels = try await CtcModels.loadDirect(from:ctc)
        let tokenizer = try await CtcTokenizer.load(from:ctc)
        let terms = try experiment.terms.map {term in
            let ids = tokenizer.encode(term)
            guard !ids.isEmpty, ids.count <= 32 else {throw Failure.invalid("Empty/oversized term tokenization")}
            return CustomVocabularyTerm(text:term,ctcTokenIds:ids)
        }
        let vocabulary = CustomVocabularyContext(terms:terms)
        let spotter = CtcKeywordSpotter(models:ctcModels,blankId:ctcModels.vocabulary.count)
        let defaultRescorer = try await VocabularyRescorer.create(spotter:spotter,vocabulary:vocabulary,ctcModelDirectory:ctc)
        let conservative = try await VocabularyRescorer.create(spotter:spotter,vocabulary:vocabulary,config:.init(shortTermCbwTaperPivot:5,spotterRescueEnabled:false),ctcModelDirectory:ctc)
        let removed = try await VocabularyRescorer.create(spotter:spotter,vocabulary:CustomVocabularyContext(terms:[]),ctcModelDirectory:ctc)
        try emit(["event":"ctc_loaded","seconds":seconds(ctcStart),"peakRSSBytes":peakRSS(),"terms":terms.map { ["text":$0.text,"ctcTokenIds":$0.ctcTokenIds ?? []] as [String:Any]}])
        for (fixture,samples,result) in baselines {
            let clock = ContinuousClock.now
            let spotted = try await spotter.spotKeywordsWithLogProbs(audioSamples:samples,customVocabulary:vocabulary)
            let ctcSeconds = seconds(clock)
            for (name,rescorer) in [("default",defaultRescorer),("conservative",conservative),("removed",removed)] {
                let rescoreStart = ContinuousClock.now
                let evidence = rescorer.ctcTokenEvaluateCandidates(transcript:result.text,tokenTimings:result.tokenTimings ?? [],logProbs:spotted.logProbs,frameDuration:spotted.frameDuration)
                guard evidence.baseText == result.text else {throw Failure.invalid("Candidate API changed original text")}
                let output = rescorer.ctcTokenRescore(transcript:result.text,tokenTimings:result.tokenTimings ?? [],logProbs:spotted.logProbs,frameDuration:spotted.frameDuration)
                try emit(["event":"rescored","id":fixture.id,"config":name,"text":output.text,"modified":output.wasModified,"ctcSeconds":ctcSeconds,"rescoreSeconds":seconds(rescoreStart),"peakRSSBytes":peakRSS(),"detections":spotted.detections.map {["term":$0.term.text,"score":$0.score,"start":$0.startTime,"end":$0.endTime] as [String:Any]},"candidates":evidence.candidates.map {c in ["base":c.basePhrase,"term":c.canonicalTerm,"vocabularyScore":c.rawVocabularyCTCScore as Any?,"originalScore":c.rawOriginalCTCScore as Any?,"boost":c.effectiveBoost as Any?,"passed":c.comparisonPassed,"outcome":String(describing:c.legacyOutcome),"reason":c.reason].mapValues {$0 ?? NSNull()} }])
            }
        }
    }
}
