# Apple Core ML speech adapter

This is Tuck's native Apple speech helper. Rust owns capture, bounded queues,
model installation and checksum verification, persistence, and lifecycle. The
helper owns one serialized FluidAudio `AsrManager`. It never downloads models.
It requires macOS 14+, Swift 6.2+ to build, and the 632,169,729-byte model set in
`models.json`. The fixed int8-linear `Encoder_v2.mlmodelc` is selected explicitly
with `.int8V2`; the original 6-bit encoder is not a fallback.

Build from the repository root:

```sh
node scripts/build-apple-speech.mjs
```

The build script clones FluidAudio v0.17.7 into a generated cache, verifies exact
commit `503b4bd1bbf7220882de39fe8ae6716aae4132da`, and applies only the tracked
manifest patch to omit the unused NeMo binary. ASR source remains unchanged.
The patch avoids SwiftPM's attempted download of an unused 83 MiB xcframework.
The built executable is copied to `src-tauri/binaries/tuck-speech`. No model
weights are fetched by the build. A developer can set `TUCK_FLUIDAUDIO_SOURCE`
to an existing exact checkout; the script rejects unexpected source changes.

During explicit model installation, the Rust owner verifies every file in
`models.json`, stages the final model directory, and runs:

```sh
src-tauri/binaries/tuck-speech --models /absolute/final/model/directory --prepare
```

This loads and optimizes Core ML models and exercises a silent prediction.
First-use optimization may take a few minutes. A `prepared` response means this
process succeeded; system cache eviction can still make a later load slower.
Prepare using the final model path. A successful preparation must precede the
owner marking an installation ready.

## Framed protocol

Start `tuck-speech --models /absolute/verified/model/directory`. The helper loads
local models and emits one stdout line:

```text
TUCK_SPEECH:{"event":"ready","engine":"coreml","computeUnits":"cpuAndNeuralEngine"}
```

Send a UTF-8 JSON header ending in one newline, immediately followed by binary
PCM bytes:

```text
{"id":1,"op":"transcribe","samples":61521}\n
<61521 little-endian Float32 samples, 16 kHz mono; no delimiter>
```

`id` is an unsigned 64-bit integer. `samples` must be 1–240,000 (at most 15
seconds). Samples must be finite and in [-1, 1]. Headers are bounded to 4,096
bytes. The reader shares one bounded buffer across headers and payloads and uses
POSIX `read`, so partial frames and multiple frames in a single write work.
Each request receives fresh decoder state. Turns shorter than 300 ms are padded
for the SDK's minimum length; response duration and token timestamps retain the
true turn bounds.

Response:

```text
TUCK_SPEECH:{"id":1,"text":"...","duration":3.8450625,"tokenTimings":[{"token":"...","startTime":0.08,"endTime":0.2,"confidence":0.9}]}
```

Errors use `{id,error}` when a request ID is known, or `{event:"error",error}`
for initialization/malformed framing. A failed inference or invalid payload ends
the worker after its error response; the Rust owner must preserve the recording
and report the failure. Diagnostics go to stderr without transcript or PCM.
Send `{"op":"shutdown"}\n` to exit normally, or close stdin. The owner should
also enforce deadlines and kill/wait during cancellation or shutdown.

## Explicit verification

With already verified models and the prepared public Float32 test fixture:

```sh
node native/apple-speech/verify.mjs /tmp/tuck-apple-speech/models \
  /tmp/tuck-verification/en-16k.f32
```

The integration verifier checks actual inference, partial/coalesced binary
framing, ordered requests, short turns, shutdown, bounded input, invalid floats,
truncated payloads, oversized headers, and missing-model failure. It does not
record audio or use accounts.

The original-encoder benchmark in `benchmarks/apple-speech` is separate from this
production model choice. See `docs/apple-speech-evaluation.md` for measured
results, conversion differences, known limits, and Apple compute-plan evidence.
