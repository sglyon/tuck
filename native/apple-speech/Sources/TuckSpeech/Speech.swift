import CoreML
import Darwin
import FluidAudio
import Foundation

private let maximumSamples = 16_000 * 15
private let maximumHeaderBytes = 4_096

private enum SpeechError: Error, LocalizedError {
    case invalid(String)
    var errorDescription: String? {
        switch self { case .invalid(let message): return message }
    }
}

/// A bounded reader owns buffering for both JSON headers and the following PCM.
/// Never mix readLine() with binary FileHandle reads: either may consume ahead.
private struct InputReader {
    private var buffered = Data()

    /// Foundation's read(upToCount:) can wait to fill the requested count on a
    /// pipe. POSIX read returns available bytes, including a short JSON-only
    /// shutdown frame, without waiting for the client to close stdin.
    private func readSome(upTo count: Int) throws -> Data {
        var data = Data(count: count)
        while true {
            let received = data.withUnsafeMutableBytes { bytes in
                Darwin.read(STDIN_FILENO, bytes.baseAddress!, count)
            }
            if received < 0 {
                if errno == EINTR { continue }
                throw SpeechError.invalid("Unable to read request stream.")
            }
            data.count = received
            return data
        }
    }

    mutating func header() throws -> Data? {
        while true {
            if let newline = buffered.firstIndex(of: 10) {
                let length = buffered.distance(from: buffered.startIndex, to: newline)
                guard length <= maximumHeaderBytes else {
                    throw SpeechError.invalid("Request header exceeds 4096 bytes.")
                }
                let line = Data(buffered.prefix(length))
                buffered.removeFirst(length + 1)
                return line
            }
            guard buffered.count <= maximumHeaderBytes else {
                throw SpeechError.invalid("Request header exceeds 4096 bytes.")
            }
            let bytes = try readSome(upTo: 4_096)
            guard !bytes.isEmpty else {
                if buffered.isEmpty { return nil }
                throw SpeechError.invalid("Incomplete request header.")
            }
            buffered.append(bytes)
        }
    }

    mutating func pcm(samples: Int) throws -> [Float] {
        let count = samples * MemoryLayout<UInt32>.size
        var payload = Data()
        payload.reserveCapacity(count)
        let prefix = min(count, buffered.count)
        payload.append(buffered.prefix(prefix))
        buffered.removeFirst(prefix)
        while payload.count < count {
            let bytes = try readSome(upTo: min(65_536, count - payload.count))
            guard !bytes.isEmpty else {
                throw SpeechError.invalid("Incomplete PCM payload.")
            }
            payload.append(bytes)
        }
        let values: [Float] = payload.withUnsafeBytes { bytes in
            (0..<samples).map { index in
                Float(bitPattern: UInt32(littleEndian: bytes.loadUnaligned(fromByteOffset: index * 4, as: UInt32.self)))
            }
        }
        guard values.allSatisfy({ $0.isFinite && abs($0) <= 1 }) else {
            throw SpeechError.invalid("PCM must contain finite Float32 samples in [-1, 1].")
        }
        return values
    }
}

private struct Request: Decodable {
    let id: UInt64?
    let op: String
    let samples: Int?
}

private func emit(_ value: [String: Any]) throws {
    let json = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    var framed = Data("TUCK_SPEECH:".utf8)
    framed.append(json)
    framed.append(10)
    try FileHandle.standardOutput.write(contentsOf: framed)
}

private func log(_ text: String) {
    try? FileHandle.standardError.write(contentsOf: Data((text + "\n").utf8))
}

@main
struct TuckSpeech {
    static func main() async {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            if arguments.count == 4 && arguments[0] == "--diarize" {
                try await diarize(models: arguments[1], audio: arguments[2], source: arguments[3])
                return
            }
            guard arguments.count == 2 || arguments.count == 3,
                  arguments[0] == "--models",
                  arguments.count == 2 || arguments[2] == "--prepare" else {
                throw SpeechError.invalid("Usage: tuck-speech --models DIRECTORY [--prepare]")
            }
            let directory = URL(fileURLWithPath: arguments[1], isDirectory: true)
            let configuration = AsrModels.defaultConfiguration()
            configuration.computeUnits = .cpuAndNeuralEngine
            // Rust verifies all pinned model hashes before spawning. This local-only
            // loader cannot download, repair, or select a different model directory.
            let models = try AsrModels.loadLocal(
                from: directory, version: .v3, configuration: configuration,
                encoderPrecision: .int8V2, encoderComputeUnits: .cpuAndNeuralEngine)
            let manager = AsrManager(config: ASRConfig(parallelChunkConcurrency: 1))
            try await manager.loadModels(models)
            if arguments.count == 3 {
                // First-install ANE optimization is normally paid by loadLocal.
                // Exercise one full prediction too so lazy preparation is paid here.
                var state = try TdtDecoderState()
                _ = try await manager.transcribe([Float](repeating: 0, count: 16_000), decoderState: &state)
                try emit(["event": "prepared", "engine": "coreml", "computeUnits": "cpuAndNeuralEngine"])
                return
            }
            try emit(["event": "ready", "engine": "coreml", "computeUnits": "cpuAndNeuralEngine"])
            var input = InputReader()
            while let line = try input.header() {
                let request = try JSONDecoder().decode(Request.self, from: line)
                if request.op == "shutdown" { return }
                guard let id = request.id, request.op == "transcribe",
                      let count = request.samples, count > 0, count <= maximumSamples else {
                    // Framing cannot be recovered safely without a valid byte count.
                    throw SpeechError.invalid("Expected transcribe with id and 1...240000 samples, or shutdown.")
                }
                do {
                    let samples = try input.pcm(samples: count)
                    var state = try TdtDecoderState()
                    // FluidAudio rejects <300 ms even though its model pads every
                    // window. Pad short final turns, retaining the true duration.
                    let inferenceSamples = count < 4_800
                        ? samples + [Float](repeating: 0, count: 4_800 - count) : samples
                    let result = try await manager.transcribe(inferenceSamples, decoderState: &state)
                    let duration = Double(count) / 16_000
                    let tokens: [[String: Any]] = (result.tokenTimings ?? []).map { token in
                        ["token": token.token, "startTime": max(0, min(duration, token.startTime)),
                         "endTime": max(0, min(duration, token.endTime)), "confidence": token.confidence]
                    }
                    let words: [[String: Any]] = buildWordTimings(from: result.tokenTimings ?? []).map { word in
                        let included = (result.tokenTimings ?? []).filter { $0.endTime >= word.startTime && $0.startTime <= word.endTime }
                        let confidence = included.isEmpty ? 0 : included.reduce(Float(0)) { $0 + $1.confidence } / Float(included.count)
                        return ["word":word.word, "startTime":max(0,min(duration,word.startTime)), "endTime":max(0,min(duration,word.endTime)), "confidence":confidence]
                    }
                    try emit(["id": id, "text": result.text, "duration": duration, "tokenTimings": tokens, "words": words])
                } catch {
                    // No transcript or PCM is included in diagnostics.
                    try emit(["id": id, "error": error.localizedDescription])
                    // A truncated binary payload can leave framing ambiguous; end
                    // this process and let the owner preserve/recover the recording.
                    return
                }
            }
        } catch {
            try? emit(["event": "error", "error": error.localizedDescription])
            log("tuck-speech: initialization or request failed.")
            exit(1)
        }
    }
}
