import CoreML
import Darwin
import FluidAudio
import Foundation

func seconds(_ start: UInt64) -> Double {
    Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000_000
}

func cpuSeconds() -> Double {
    var usage = rusage()
    getrusage(RUSAGE_SELF, &usage)
    return Double(usage.ru_utime.tv_sec + usage.ru_stime.tv_sec)
        + Double(usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) / 1_000_000
}

func memory() -> [String: UInt64] {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let result = withUnsafeMutablePointer(to: &info) { pointer in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    guard result == KERN_SUCCESS else { return [:] }
    return ["rss_bytes": info.resident_size, "physical_footprint_bytes": info.phys_footprint]
}

func emit(_ event: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: event, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
    fflush(stdout)
}

@available(macOS 14.4, *)
func computePlans(directory: URL, encoder: String) async throws {
    for component in ["Preprocessor", encoder, "Decoder", "JointDecisionv3"] {
        let configuration = MLModelConfiguration()
        configuration.computeUnits = component == "Preprocessor" ? .cpuOnly : .cpuAndNeuralEngine
        let plan = try await MLComputePlan.load(
            contentsOf: directory.appendingPathComponent(component + ".mlmodelc"),
            configuration: configuration)
        var counts: [String: Int] = [:]
        func visit(_ block: MLModelStructure.Program.Block) {
            for operation in block.operations {
                let device: String
                switch plan.deviceUsage(for: operation)?.preferred {
                case .cpu: device = "cpu"
                case .gpu: device = "gpu"
                case .neuralEngine: device = "neuralEngine"
                case nil: device = "unreported"
                @unknown default: device = "unknown"
                }
                counts[device, default: 0] += 1
                for child in operation.blocks { visit(child) }
            }
        }
        if case let .program(program) = plan.modelStructure {
            for function in program.functions.values { visit(function.block) }
        }
        try emit(["event": "anticipated_compute_plan", "component": component,
                  "preferred_operation_counts": counts])
    }
}

@main
struct AppleSpeechBenchmark {
    static func main() async throws {
        let args = CommandLine.arguments
        guard args.count >= 4 else {
            fatalError("Usage: AppleSpeechBenchmark MODEL_DIRECTORY WAV_FILE ane|cpu|plan [REPETITIONS] [int8-v2]")
        }
        let precision: ParakeetEncoderPrecision = args.count > 5 && args[5] == "int8-v2" ? .int8V2 : .int8
        if args[3] == "plan" {
            if #available(macOS 14.4, *) {
                try await computePlans(directory: URL(fileURLWithPath: args[1]),
                                       encoder: precision == .int8V2 ? "Encoder_v2" : "Encoder")
                return
            }
            fatalError("Compute-plan inspection requires macOS 14.4+")
        }
        let started = DispatchTime.now().uptimeNanoseconds
        let initialCPU = cpuSeconds()
        let units: MLComputeUnits = args[3] == "cpu" ? .cpuOnly : .cpuAndNeuralEngine
        let repetitions = args.count > 4 ? Int(args[4])! : 5
        let configuration = AsrModels.defaultConfiguration()
        configuration.computeUnits = units
        try emit(["event": "process_start", "memory": memory(), "requested_compute": args[3]])
        let models = try AsrModels.loadLocal(
            from: URL(fileURLWithPath: args[1]), version: .v3,
            configuration: configuration, encoderPrecision: precision,
            encoderComputeUnits: units)
        let manager = AsrManager()
        try await manager.loadModels(models)
        try emit(["event": "loaded", "wall_seconds": seconds(started),
                  "cpu_seconds": cpuSeconds() - initialCPU, "memory": memory(),
                  "preprocessor_compute_raw": models.preprocessor.configuration.computeUnits.rawValue,
                  "encoder_compute_raw": models.encoder!.configuration.computeUnits.rawValue,
                  "decoder_compute_raw": models.decoder.configuration.computeUnits.rawValue,
                  "joint_compute_raw": models.joint.configuration.computeUnits.rawValue])
        let conversionStart = DispatchTime.now().uptimeNanoseconds
        let samples = try AudioConverter().resampleAudioFile(URL(fileURLWithPath: args[2]))
        try emit(["event": "audio_loaded", "samples": samples.count,
                  "audio_seconds": Double(samples.count) / 16000,
                  "wall_seconds": seconds(conversionStart)])
        for iteration in 0..<repetitions {
            var state = try TdtDecoderState()
            let inferenceStart = DispatchTime.now().uptimeNanoseconds
            let inferenceCPU = cpuSeconds()
            let result = try await manager.transcribe(samples, decoderState: &state)
            let elapsed = seconds(inferenceStart)
            guard result.text.lowercased().contains("what your country can do for you") else {
                fatalError("Unexpected transcript: \(result.text)")
            }
            try emit(["event": "transcribed", "iteration": iteration,
                      "wall_seconds": elapsed, "cpu_seconds": cpuSeconds() - inferenceCPU,
                      "memory": memory(), "text": result.text,
                      "rtf": elapsed / (Double(samples.count) / 16000)])
        }
        try emit(["event": "finished", "wall_seconds": seconds(started),
                  "cpu_seconds": cpuSeconds() - initialCPU, "memory": memory()])
    }
}
