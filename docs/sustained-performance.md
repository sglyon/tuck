# Sustained native transcription measurement

This is a bounded development check for the confirmed target: a comfortable 16 GB laptop alongside an active video call. It does **not** qualify that target. The available machine has 32 GiB of RAM, and the test includes neither a video call nor the app's WebKit UI.

The harness in `scripts/measure-native.mjs` drives the release Rust diagnostic runtime and the speech helper extracted from the app bundle. A disposable synthetic system-audio process repeats the public Kennedy PCM in real time with one second of silence between repetitions. The [publisher's fixture](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/test_wavs/en.wav) was already provisioned and resampled once; the harness records its PCM SHA-256. No microphone, real system audio, account, hosted inference, or local LLM is used. The optional speaker model is not installed in this test library. The source and library are temporary and removed after shutdown; only measurements are retained.

The check requires meaningful repeated transcription, successful recording completion, enough owned WAV evidence for the elapsed time, no reported engine errors, and release of the capture and Core ML child processes after stop. It samples only the runtime and its direct children every five seconds. The fake PCM source is marked separately and excluded from the Rust/Core ML summaries.

## Results

Run completed on October 23, 2026 at 07:18 EDT. Hardware was read from `sysctl`, `sw_vers` and `uname`: Mac18,5, Apple M6, 34,359,738,368 bytes RAM (32 GiB), macOS 27.0.1 arm64. Raw observations and exact runtime/helper/PCM hashes are in [measurement.json](../benchmarks/native-runtime/measurement.json).

| Observation | Result |
| --- | ---: |
| Recording wall time | 180.071 s |
| Transcript segments / reported engine errors | 37 / 0 |
| Owned WAV bytes | 5,708,844 |
| Approximate segment-arrival delay, median / p95 / maximum | 1.343 / 2.017 / 2.097 s |
| Rust runtime peak / final recording RSS | 33.20 / 33.20 MiB |
| Core ML helper peak / final recording RSS | 91.16 / 87.00 MiB |
| Rust process CPU time delta | 1.89 s across 180.03 s (1.05% of one core) |
| Core ML process CPU time delta | 2.47 s across 180.03 s (1.37% of one core) |
| Stop/drain wall time | 0.071 s |
| Capture and Core ML child processes after stop | None |

After the first 30 seconds, Rust RSS stayed between 33.13 and 33.20 MiB; helper RSS ranged from 84.84 to 91.16 MiB. That is no sign of runaway growth in this brief workload, not proof of leak freedom. Successful completion and repeated expected words were asserted. The persisted WAV's size was checked against wall time, and the temporary library was removed after orderly shutdown.

**Startup took 82.97 seconds before recording began.** The test used a new temporary symlink alias to existing verified model files and a shipped helper. It paid a preparation-like startup cost despite earlier model preparation elsewhere. Cache-path sensitivity and other Core ML cache factors were not isolated. These figures are not a warm-start claim. Production must prepare at the final installed model path before marking the model ready; the canonical-path follow-up below records a focused repeated-start check. OS cache eviction can still make startup slower.

The two process peaks must not be advertised as “124 MB total app memory”: the GUI, WebKit processes, system services, speaker model, local LLM and video call were excluded. This result provides a small-workload resource baseline, not a pass for the 16 GB product target.

## Interpretation

- Per-process RSS is resident mappings, not total physical memory. Shared pages can be counted twice, while allocations held by Core ML, ANE or compiler services outside these processes are excluded. Do not treat the sum as the app's total machine footprint.
- CPU time deltas are divided by elapsed sampled time and expressed as a percentage of one CPU core. The separate mean of `ps %CPU` is labeled as the mean reported sample, not assumed to be interval CPU use. Accelerator execution is outside these process CPU figures, so they are not energy measurements.
- Approximate transcription delay is segment arrival wall time minus the segment's audio end time, relative to the start-recording response. It includes VAD finalization, buffering and inference. The response follows actual activation by a small unmeasured delay. Repeated source timers can drift, and this clean repeated sample is not an accuracy or conversational-turn corpus.
- Root was also performing native UI acceptance work on the same development host. This was not a quiet benchmark, and its UI processes are outside this harness's process measurements.
- Three minutes can expose gross backlog, runaway buffering and failed cleanup. It does not establish leak freedom, hours-long behavior, call responsiveness, sustained thermal behavior or operation under a 16 GB system's memory pressure.

## Reproduce

Build the native helpers and release diagnostic runtime, explicitly provision the verified Core ML V2 model set and public 16 kHz Float32 fixture, then run on macOS:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --release --features diagnostics --bin tuck-runtime
node scripts/measure-native.mjs \
  --engine src-tauri/target/release/tuck-runtime \
  --models /absolute/verified/models \
  --pcm /absolute/public-16k.f32 \
  --speech src-tauri/target/release/bundle/macos/Tuck.app/Contents/Resources/binaries/tuck-speech \
  --seconds 180 --out benchmarks/native-runtime/measurement.json
```

Process inspection requires permission to inspect these owned processes on the host. The harness accepts 15–600 seconds; longer real-device acceptance should be a separately designed test, including the full UI, microphone plus system audio, speaker model and an actual active call.

## Prepared startup through fresh aliases

A focused follow-up ran after the native adapter was changed to canonicalize the model directory before both explicit preparation and normal loading. Read-only review found no blocking issue in the small change; the unit regression covers two symlink aliases and rejects missing/non-directory paths. The earlier three-minute measurement and its runtime hash remain unchanged.

Using the same bundled helper, explicit preparation at `/private/tmp/tuck-rust-coreml-models/parakeet-v3-coreml` completed in **0.200 s**. Two separate temporary library aliases then resolved to that same physical model directory and started fresh release-runtime processes. Both recorded public PCM for 15 seconds, verified real expected words, reported zero engine errors and cleaned up their child processes. Observed start-recording times were **1.400 s** and **1.450 s**; they produced 3 and 4 segments respectively.

Exact hashes and observations are in [warm-start.json](../benchmarks/native-runtime/warm-start.json). The rebuilt runtime hash differs from the original three-minute test; the helper hash is unchanged. `scripts/verify-native-warm.mjs` reproduces the explicit canonical preparation plus two short runs using the same argument names as `measure-native.mjs` (and an optional `--out`).

This demonstrates two successful prepared starts through fresh aliases on this host. It does not isolate the cause of the earlier 82.97-second startup: canonicalization, preparation state and opaque Core ML cache behavior were not held constant in a controlled A/B experiment. OS cache eviction or another cold model preparation can still make startup slower. These short checks do not qualify a 16 GB laptop, call responsiveness, long meetings or total memory usage.
