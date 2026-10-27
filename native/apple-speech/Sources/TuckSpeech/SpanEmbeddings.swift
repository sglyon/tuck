import CoreML
import FluidAudio
import Foundation

private struct EmbeddingSpan: Decodable {
    let id: String
    let start: Double
    let end: Double
}

// Exact audio only: tiling fills the fixed model window without borrowing a
// neighboring speaker or letting zero padding dominate log-mel normalization.
func embedSpans(models path: String, audio: String, source: String) throws {
    var input = Data()
    while let chunk = try FileHandle.standardInput.read(upToCount: 4096), !chunk.isEmpty {
        input.append(chunk)
        guard input.count <= 65536 else { throw CocoaError(.fileReadCorruptFile) }
    }
    let spans = try JSONDecoder().decode([EmbeddingSpan].self, from: input)
    guard spans.count <= 128, Set(spans.map(\.id)).count == spans.count,
          spans.allSatisfy({ $0.id.utf8.count <= 128 && $0.start.isFinite && $0.end.isFinite && $0.start >= 0 && $0.end > $0.start && $0.end <= 7200.3 && $0.end - $0.start <= 2.2 }) else {
        throw CocoaError(.fileReadCorruptFile)
    }
    let directory = URL(fileURLWithPath: path, isDirectory: true)
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuAndNeuralEngine
    let fbank = try MLModel(contentsOf: directory.appendingPathComponent("FBank.mlmodelc"), configuration: configuration)
    let embedding = try MLModel(contentsOf: directory.appendingPathComponent("Embedding.mlmodelc"), configuration: configuration)
    guard let audioShape = fbank.modelDescription.inputDescriptionsByName["audio"]?.multiArrayConstraint?.shape,
          let weightShape = embedding.modelDescription.inputDescriptionsByName["weights"]?.multiArrayConstraint?.shape,
          audioShape.map(\.intValue) == [1, 1, 160000], weightShape.map(\.intValue) == [1, 589] else {
        throw CocoaError(.fileReadCorruptFile)
    }
    let weights = try MLMultiArray(shape: weightShape, dataType: .float32)
    for index in 0..<weights.count { weights[index] = 1 }
    let samples = try AudioConverter().resampleAudioFile(path: audio)
    guard samples.count <= 16000 * 7200 else { throw CocoaError(.fileReadTooLarge) }
    var results: [[String: Any]] = []
    for span in spans {
        let first = Int((span.start * 16000).rounded())
        let last = min(samples.count, Int((span.end * 16000).rounded()))
        guard first >= 0, first < last else { continue }
        let window = try MLMultiArray(shape: audioShape, dataType: .float32)
        let pointer = window.dataPointer.assumingMemoryBound(to: Float.self)
        for index in 0..<window.count { pointer[index] = samples[first + index % (last - first)] }
        let features = try fbank.prediction(from: MLDictionaryFeatureProvider(dictionary: ["audio": window]))
        guard let banks = features.featureValue(for: "fbank_features")?.multiArrayValue else { throw CocoaError(.fileReadCorruptFile) }
        let result = try embedding.prediction(from: MLDictionaryFeatureProvider(dictionary: ["fbank_features": banks, "weights": weights]))
        guard let vector = result.featureValue(for: "embedding")?.multiArrayValue, vector.count == 256 else { throw CocoaError(.fileReadCorruptFile) }
        let values = (0..<256).map { vector[$0].floatValue }
        guard values.allSatisfy(\.isFinite) else { throw CocoaError(.fileReadCorruptFile) }
        results.append(["id": span.id, "embedding": values])
    }
    let response = try JSONSerialization.data(withJSONObject: ["source": source, "spans": results], options: [.sortedKeys])
    try FileHandle.standardOutput.write(contentsOf: Data("TUCK_SPAN_EMBEDDINGS:".utf8) + response + Data([10]))
}
