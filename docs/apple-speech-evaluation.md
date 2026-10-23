# Native Apple speech evaluation

Verified on 2026-10-23 using a Mac mini Mac18,5, Apple M6, 32 GiB RAM,
macOS 27.0.1 arm64, Apple Swift 6.4. These measurements are for this host and one
public 3.845-second speech sample; they do not establish performance on an 8 GiB
M1 or an iPhone.

The benchmark source, exact model hashes, build workaround, and commands are in
[`benchmarks/apple-speech`](../benchmarks/apple-speech/README.md). No microphone,
system audio, user recording, account, or cloud inference was used. Audio fixture
SHA-256: `148b936b43ce7c546a866e64da059f0458aee2d65e617f16e9d94f06e8d99ed6`.

## Reproducible dependency and model selection

The latest official release observed was
[FluidAudio v0.17.7](https://github.com/FluidInference/FluidAudio/releases/tag/v0.17.7),
released 2026-10-08. Its annotated tag resolves to source commit
`503b4bd1bbf7220882de39fe8ae6716aae4132da`. The benchmark builds release mode
against that commit and uses `AsrModels.loadLocal`, so inference never downloads
assets or resolves mutable model branches.

The [FluidInference Parakeet v3 Core ML model](https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml/tree/7dd20fe6b1797d35f5e3307e8b1732d9a178edfe)
is pinned to revision `7dd20fe6b1797d35f5e3307e8b1732d9a178edfe`. Only the
preprocessor, encoder, decoder, JointDecisionv3, and vocabulary were downloaded:
483,105,645 bytes. Every upstream LFS SHA-256 or ordinary Git blob hash was
verified, and a SHA-256 manifest is saved with the benchmark. The model card
declares CC-BY-4.0; the SDK code uses Apache-2.0. Model attribution must accompany
distribution separately from SDK attribution.

The SDK calls this encoder precision `.int8`, but its pinned source explains
that it selects the original 6-bit palettized `Encoder.mlmodelc`. This is the same
NVIDIA Parakeet TDT v3 base family as the earlier ONNX test, with a different
conversion, precision, frontend, decoding implementation, and component layout.
It is not a controlled language-only or identical-weights runtime comparison.

`traits: []` in this release disables NeMo text normalization. It still compiles
TTS and diarization source. The TTS/Diarizer opt-out traits documented on upstream
main were added after this release. SwiftPM stalled while fetching the declared
83 MiB NeMo artifact despite the opt-out. A benchmark-only manifest patch removes
that unused binary target and dependency reference; the exact patch is saved.
No inference source was changed. The patched build completed in 71.06 seconds.
The production build now applies and verifies the same manifest-only patch
explicitly; the upstream packaging issue remains separate from inference. The resulting executable was approximately 9.7 MiB unstripped, linked to
system Apple frameworks, and does not load Python, PyTorch, or ONNX Runtime.

## Compute selection and evidence

The benchmark explicitly sets CPU plus Neural Engine on encoder, decoder, and
joint, with CPU-only preprocessing. It also runs the exact same Core ML assets
with CPU-only on every component. Each loaded model's `configuration.computeUnits`
is emitted as evidence that the requested settings were applied.

Apple defines
[`.cpuAndNeuralEngine`](https://developer.apple.com/documentation/coreml/mlcomputeunits/cpuandneuralengine)
as allowing CPU and Neural Engine while excluding GPU. This allows fallback;
it does not force all operators onto ANE. A separate
[`MLComputePlan`](https://developer.apple.com/documentation/coreml/mlcomputeplan-1w21n)
inspection reports anticipated per-operation placement. The plan is a prediction
of placement, not proof of an executed hardware trace. No Instruments/ANE runtime
trace or power measurement was collected. Process CPU time excludes accelerator
execution and cannot be read as energy usage.

The local plan reported the encoder with 1,378 operations preferring ANE and 7
preferring CPU. The preprocessor (51), decoder (24), and joint (24) reported
CPU-preferred operations. Other operations were unreported, often constants.
Allowing ANE for decoder and joint did not make their plan prefer ANE on this
host. No operation preferred GPU.

## Measured results

These timing and memory results use the original 6-bit palettized encoder, not
the int8-linear production selection described below.

All runs produced: “Ask not what your country can do for you. Ask what you can do
for your country.” Each process loaded one model manager and decoded the same
file six times, with a fresh decoder state each time. Warm medians exclude the
first decode and contain five samples. Audio conversion took approximately
10 ms and is excluded from decode times. AVAudioConverter emitted 61,522 samples
(3.845125 seconds); the original fixture is nominally 3.845 seconds.

| Metric | CPU + ANE allowed, initial preparation | CPU + ANE allowed, cached process | CPU-only, cached process |
| --- | ---: | ---: | ---: |
| Load models, wall time | 60.173 s | 0.087 s | 0.017 s |
| First decode, wall time | 50.6 ms | 42.5 ms | 223.0 ms |
| Warm decode, median wall time | 37.1 ms | 32.3 ms | 173.5 ms |
| Warm decode, median process CPU time | 33.9 ms | 28.0 ms | 207.0 ms |
| Process RSS after warm decode | 94.3 MiB | 76.9 MiB | 637.9 MiB |
| Peak process RSS (`time -l`) | 497.7 MiB | 77.0 MiB | 637.9 MiB |
| Peak process physical footprint (`time -l`) | 63.3 MiB | 38.6 MiB | 71.0 MiB |

On the identical Core ML assets and input PCM, the cached warm median with ANE
permitted was approximately 5.37 times faster than CPU-only. Core ML's CPU-only
first run loaded in 0.577 s and peaked at 1,001.8 MiB RSS; subsequent process
startup reused prepared assets. Raw JSONL, `/usr/bin/time -l` output, and an exact
numeric summary are saved with the benchmark.

**Memory is process accounting, not total system or accelerator memory.** RSS
includes resident mapped pages, while physical-footprint accounting differs;
neither number represents all Apple runtime services or unified accelerator
allocations. ANECompilerService was busy during first-use preparation. Its CPU
and memory are outside this process's figures. The 60-second setup cost matters
even though process CPU time for that load was only 2.49 seconds. No system-wide
peak memory delta, energy, or long-running thermal behavior was measured.

The earlier Python/sherpa test's 1.434 s and 1.82 GiB peak included checksum
verification, resampling, VAD, ASR, and speaker extraction. This native benchmark
measures ASR and audio conversion, so those end-to-end numbers cannot establish
an equivalent-pipeline speedup. The Core ML CPU-versus-ANE control above is the
cleaner acceleration comparison. One clean quotation does not establish model
accuracy on meetings, accents, noise, or overlapping voices.

## Production encoder and native helper

Production explicitly selects `.int8V2`, the int8-linear `Encoder_v2.mlmodelc`.
The original conversion has a documented right-context corruption case;
[upstream PR 872](https://github.com/FluidInference/FluidAudio/pull/872) adds the
separate fixed encoder without changing the SDK default. The production manifest
in `native/apple-speech/models.json` pins the same HF revision and verifies all
20 files: **632,169,729 bytes**, replacing the original encoder's files while
reusing identical preprocessor, decoder, joint, and vocabulary. This model change
does not inherit the original encoder's benchmark numbers.

The native `tuck-speech` executable is implemented under `native/apple-speech`.
`scripts/build-apple-speech.mjs` builds the exact pinned SDK with the recorded
manifest-only NeMo opt-out patch and rejects unexpected source modifications.
No unverified Rust wrapper is required. The helper accepts a verified local model
directory, emits framed ready/transcript/error JSON, and consumes bounded binary
16 kHz Float32 PCM from Rust. It serializes one model manager, bounds requests to
15 seconds, validates finite sample values, starts fresh decoder state per turn,
and supports explicit shutdown. The stream reader handles fragmented and
coalesced frames without relying on pipe reads filling a requested byte count.

Explicit `--prepare` loads and optimizes the model and exercises a silent decode.
The first int8V2 preparation here took **87.49 seconds** and peaked at
669,630,464 bytes process RSS (638.6 MiB). As with the original preparation,
compiler-service costs are outside those process figures. The production install
flow should say “Preparing” and run this step at the final installed model path
before marking the model ready. Cache path sensitivity was not isolated, and OS
cache eviction can still make later startup slower.

The final int8V2 helper passed actual speech transcription, partial and combined
frames, ordered multiple requests, short turns, shutdown without EOF, bounds,
invalid floats, truncated payload, oversized header, and missing-model checks.
The full protocol verification completed in 0.89 seconds after preparation;
this is a test completion time, not an isolated inference benchmark.

The reporter's public regression B (15 seconds) and control C (11 seconds) from
[issue 760](https://github.com/FluidInference/FluidAudio/issues/760) both retained
the expected Ukrainian phrase “фоновий процес” without the reported corruption.
The audio is held only in `/tmp` and is not redistributed. Original WAV SHA-256:

- B: `9dc70b0a0eff8b6ee4298590c325f097283aead9faa7862e3daab6af41a5d008`
- C: `3fcef5b1f07519487fa7304420a4ec061ea6282949be92057481e93c8725a2f0`

These targeted checks are not a broad accuracy evaluation or a new int8V2 latency
claim. No iPhone, noisy meeting, sustained thermal, or battery test was run.

The separate int8V2 compute plan also reported 1,378 encoder operations
preferring ANE and 7 preferring CPU. Preprocessor, decoder, and joint preferred
CPU, with no GPU preference. This confirms anticipated accelerator eligibility
for the selected production conversion; it does not replace a runtime hardware
trace or establish the original conversion's speed for int8V2.

## Architecture implications

A Rust-owned engine uses this Swift/Core ML backend through a persistent native
helper. Keep bounded PCM buffers, cancellation, transcript
events, timestamp units, model lifecycle, and error handling in a stable Rust
interface. Keep `AsrManager`, Core ML objects, and Swift actor isolation inside
the Apple adapter. The helper exchanges framed PCM and JSON events without
serializing model tensors. An in-process C ABI could avoid the IPC copy but would
need explicit buffer ownership and callback threading. The timing table above
benchmarks native Swift inference separately from the integrated Rust pipeline.

The [official FluidAudio README](https://github.com/FluidInference/FluidAudio)
lists a Rust/Tauri wrapper, `fluidaudio-rs`. It is a candidate bridge to inspect
and pin, rather than a reason to rewrite Core ML execution in Rust. A wrapper's
existence does not establish its cancellation, streaming, deployment, or memory
behavior for Tuck.

The pinned package supports macOS 14 and iOS 17. That makes the Swift adapter a
plausible shared Apple inference backend for macOS and iOS. Android, Windows, and
Linux need separate inference adapters; shared Rust orchestration can remain.
iOS capture permissions, background execution, audio routing, lifecycle, thermal
behavior, and end-to-end memory still need device verification.

Parakeet v3 is batch/windowed ASR. The 110M hybrid TDT/CTC model is a smaller,
separate English option with a fused frontend. Parakeet EOU 120M is the separate
cache-aware English streaming option with end-of-utterance support. Neither was
downloaded or benchmarked here; their advertised memory/speed figures are not
local evidence. Choosing either trades multilingual coverage and model behavior
for a different product profile and needs an accuracy/capture benchmark.
