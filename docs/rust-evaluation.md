# Native Rust speech evaluation

Measured 2026-10-23 on this Mac18,5, Apple M6, 32 GiB RAM, macOS 27.0.1 arm64. These results do not establish performance on an 8 GiB M1, Intel Mac, phone, or NVIDIA GPU. The public Kennedy fixture is the only audio used.

## What changing language changes

The current Python adapter delegates model execution and transducer decoding to native sherpa-onnx C++ and ONNX Runtime. The official Rust `sherpa-onnx` crate wraps the same C API. Moving orchestration into Rust removes Python packaging, interpreter startup, and Python application state; it does not remove the model's weights, tensors, or native execution memory. A meaningful comparison must hold model export, PCM samples, thread count, and native library build constant. The official [Rust examples](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/rust-api-examples/Cargo.toml) pin the same 1.13.8 family as the current Python adapter.

The isolated benchmark in [benchmarks/rust-speech](../benchmarks/rust-speech/README.md) tests the existing verified model without changing the application. The full 487 MB model archive was not downloaded again. Cargo fetched small Rust dependencies and the official native runtime package. Rust 1.99.1 compiles a roughly 25 MB static benchmark executable.

## Identical-model CPU results

Both harnesses use sherpa-onnx 1.13.8, NVIDIA Parakeet TDT 0.6B v3 int8, greedy decoding, 16 kHz mono float32 PCM, and two CPU threads. The original 24 kHz fixture was resampled once with libsoxr and the resulting bytes reused verbatim. The 61,521-sample PCM SHA-256 is `01ce3dfe8a93bd58c9931e3a0d74c9c92330582455ff517f03e7547f5b510583`; duration is 3.8450625 seconds. Eight copies represent 30.7605 seconds. No VAD, speaker model, capture, integrity checking, IPC, or UI is included.

| Measurement | Rust / official static libraries | Rust / wheel native libraries | Python / installed wheel libraries |
| --- | ---: | ---: | ---: |
| Model load, median of 3 processes | 0.514 s | 0.696 s | 0.615 s |
| First short decode, median of 3 | 0.430 s | 0.148 s | 0.137 s |
| Model load + first decode | 0.944 s | 0.844 s | 0.751 s |
| Warm short decode, median of 15 | 0.427 s | 0.147 s | 0.130 s |
| Eight short chunks, median of 3 | 3.408 s | 1.097 s | 1.042 s |
| One joined 30.7605 s input, median of 3 | 3.341 s | 1.083 s | 1.010 s |
| Peak RSS, full workload | 1.305 GiB | 1.992 GiB | 2.015 GiB |
| Peak RSS, short clip only | Not measured | 1.793 GiB | 1.823 GiB |

All runs produced the expected Kennedy quotation; repeated short decodes were identical. A quiet measurement window excluded other agent builds and inference. Peak RSS was measured with `/usr/bin/time -l` outside the sandbox because its kernel statistics are blocked in the sandbox. Process start/import time is included in the external process duration but excluded from the table's internal load/decode timers. Filesystem caches were potentially warm. These are repeatability checks on one fixture, not a representative accuracy corpus or hours-long memory test.

The official static package is slower but uses less peak RSS than the installed wheel in this experiment. That result must not be attributed to Rust overhead: the native library artifacts differ. Both ONNX Runtime artifacts contain version 1.28.2; version equality does not establish equal build options or kernels. The static archive SHA-256 is `589e781d544d85267be5d7e74eae68fddd40ba41bf27e9961f867378170f8802`; the wheel's runtime dylib SHA-256 is `b0613d0ae53199a83b05fa48e169211498e9d40d54beaa372068ebe5ec5b0929`.

The third column links Rust against the wheel's C API library and the exact ONNX Runtime dylib with that hash. No Python interpreter runs in the Rust process. Its warm short decode was about 13.5% slower than the earlier Python passes, while joined throughput differed by about 7%. The bindings still use different entry points (C API versus pybind), and these passes were measured in a later quiet window, so smaller timing differences should not be assigned entirely to language overhead. The roughly 3.3x static-versus-Python gap largely disappears when using the same native runtime build.

Short-only peak RSS differs by approximately 31 MiB between Rust/shared (1,925,300,224 bytes) and Python (1,957,675,008 bytes). The model/native execution footprint accounts for nearly all the approximately 1.8 GiB peak. Removing Python alone does not solve that footprint. Full-workload Rust/shared peak was the median of three measured processes, 2,138,669,056 bytes; the separate Python process peaked at 2,163,064,832 bytes. Static peak was 1,400,995,840 bytes. The earlier 1.82 GiB report measured a different, short full-pipeline workload and cannot be compared directly with these 30-second peaks. The raw numeric observations and fixture/model metadata are preserved in [measurements.json](../benchmarks/rust-speech/measurements.json).

## Acceleration and fallback

The official sherpa Rust crate supports static/shared linking; it does not enable a CUDA runtime merely by changing a provider string. A CUDA-enabled native build and compatible NVIDIA drivers/runtime libraries are needed on supported hosts. CUDA does not execute on Apple GPUs. The upstream [provider/session implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/session.cc) logs a CPU fallback when a requested CUDA provider is absent. Its CoreML branch calls the Apple provider factory without checking that call's returned status. Provider selection therefore needs verification against the actual linked runtime and an inference profile; a successful transcription is insufficient proof of GPU or Neural Engine execution. Unsupported provider names also [fall back to CPU](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/provider.cc).

`parakeet-rs = 0.4.2` is a direct Rust speech implementation around ONNX Runtime (`ort = 2.0.0-rc.15`) with an experimental alternative Burn 0.22 backend. Its published crate source is revision `6ad1501898842fda76f86f7bcad6b361daf5df9e`. ONNX provider features include CUDA, TensorRT, CoreML, DirectML, MIGraphX, OpenVINO, WebGPU, and NNAPI; feature availability does not supply the vendor's drivers or guarantee compatible kernels. The [execution implementation](https://github.com/altunenes/parakeet-rs/blob/6ad1501898842fda76f86f7bcad6b361daf5df9e/src/execution.rs) registers CPU after the accelerated providers, and only marks the CPU provider registration as an error on failure. Accelerator registration failure can therefore leave a successful CPU session.

The crate's CoreML caveat says dynamic graphs can run slower and execute claimed nodes on CPU; its source also warns WebGPU may produce incorrect results. Its separate Burn `metal` feature targets Apple GPUs through wgpu/MSL, while `burn-cuda` and `burn-rocm` use Burn's vendor backends. The [README labels the pure Rust route experimental](https://github.com/altunenes/parakeet-rs/blob/6ad1501898842fda76f86f7bcad6b361daf5df9e/README.md). These are supported project options and upstream caveats, not hardware paths benchmarked here.

The existing sherpa export uses separate `decoder.int8.onnx`, `joiner.int8.onnx`, and `tokens.txt`. `parakeet-rs` TDT expects an encoder plus a combined decoder/joint and `vocab.txt`. Its Burn route expects fp32 model exports. Those model layouts cannot be made interchangeable by renaming files. A comparison would need another export/download and would change more than the orchestration language, so no additional model weights were fetched for this evaluation.

## FluidAudio Rust wrapper assessment

The official [FluidInference/fluidaudio-rs](https://github.com/FluidInference/fluidaudio-rs) repository is a Rust API over a Swift C bridge, with CoreML/Accelerate/Metal underneath. This is a native execution path without a Python interpreter. The newest published crates.io version returned by `cargo info` during verification was 0.14.1, revision `f9f7d10c840eac33451093bdd4d6c58ba3d08f47`; it pins Swift FluidAudio exactly to 0.14.1. The repository's [main manifest](https://github.com/FluidInference/fluidaudio-rs/blob/main/Cargo.toml) reported 0.14.8. The concurrent Apple evaluation uses newer Swift FluidAudio 0.17.7, so its results should not be presented as a tested 0.14.1 Rust wrapper integration.

The wrapper is a useful reference, with raw 16 kHz sample transcription, streaming sessions, VAD, diarization, and destruction of native handles. Several integration details need work for Tuck:

- Published `init_asr()` calls `AsrModels.downloadAndLoad()` without an explicit local-model-path API. Tuck requires an explicit verified install and offline loading, so a bridge extension is needed.
- Rust's public methods block while Swift `Task` operations complete through a semaphore. Invoke them on a dedicated worker, serialize use of mutable decoder state, and add a cancellation/lifecycle policy.
- The wrapper declares its raw bridge handle `Send` and `Sync`; that alone does not prove concurrent calls into mutable Swift state are safe.
- Its build script invokes host `swift build` and links Apple frameworks. The declared iOS support does not establish working Cargo/Tauri iOS cross compilation or application signing. These require target-aware build work and device testing. Windows and Linux need another adapter.

A small locally owned Swift C bridge pinned to the actually tested FluidAudio version can expose verified local loading and a narrow serialized ASR API. Reusing the official wrapper is also reasonable after those gaps are addressed and tested; adopting its default downloading behavior would regress the current install policy.
