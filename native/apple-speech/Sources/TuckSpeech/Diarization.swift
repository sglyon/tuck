import CoreML
import FluidAudio
import Foundation

// Local-only loader: installing models is an explicit Rust operation. Inference
// must never call prepareModels/load(from:), which can download missing files.
func diarize(models path: String, audio: String, source: String, includeChunks: Bool = false) async throws {
    let directory = URL(fileURLWithPath: path, isDirectory: true)
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuAndNeuralEngine
    func load(_ name: String) throws -> MLModel {
        try MLModel(contentsOf: directory.appendingPathComponent(name + ".mlmodelc"), configuration: configuration)
    }
    let parameters = try JSONSerialization.jsonObject(with: Data(contentsOf: directory.appendingPathComponent("plda-parameters.json"))) as? [String: Any]
    guard let tensors = parameters?["tensors"] as? [String: Any],
          let psi = tensors["psi"] as? [String: Any],
          let encoded = psi["data_base64"] as? String,
          let data = Data(base64Encoded: encoded), !data.isEmpty, data.count % 4 == 0 else {
        throw CocoaError(.fileReadCorruptFile)
    }
    let values: [Double] = data.withUnsafeBytes { bytes in
        (0..<(data.count / 4)).map { Double(Float(bitPattern: UInt32(littleEndian: bytes.loadUnaligned(fromByteOffset: $0 * 4, as: UInt32.self)))) }
    }
    let models = try OfflineDiarizerModels(segmentationModel: load("Segmentation"), fbankModel: load("FBank"), embeddingModel: load("Embedding"), pldaRhoModel: load("PldaRho"), pldaPsi: values, compilationDuration: 0)
    let manager = OfflineDiarizerManager(config: OfflineDiarizerConfig(exposeChunkEmbeddings: includeChunks))
    manager.initialize(models: models)
    let samples = try AudioConverter().resampleAudioFile(path: audio)
    let result = try await manager.process(audio: samples)
    let turns: [[String: Any]] = result.segments.map {
        ["speakerId": $0.speakerId, "start": $0.startTimeSeconds, "end": $0.endTimeSeconds, "quality": $0.qualityScore]
    }
    var response: [String: Any] = ["source": source, "segments": turns, "profiles": result.speakerDatabase ?? [:]]
    if includeChunks {
        response["chunks"] = (result.chunkEmbeddings ?? []).map {
            ["speakerId": $0.speakerId, "index": $0.chunkIndex, "start": $0.startTimeSeconds, "end": $0.endTimeSeconds, "embedding": $0.embedding256] as [String: Any]
        }
    }
    let payload = try JSONSerialization.data(withJSONObject: response, options: [.sortedKeys])
    try FileHandle.standardOutput.write(contentsOf: Data("TUCK_DIARIZATION:".utf8) + payload + Data([10]))
}
