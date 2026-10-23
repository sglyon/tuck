# Apple speech benchmark

Standalone verification of FluidAudio/Core ML Parakeet v3 on the same public speech
fixture used by `scripts/verify-speech.py`. It does not change Tuck's app engine or
capture any live audio. Python is used only by the explicit model downloader, never
for inference.

FluidAudio is pinned to v0.17.7's commit
`503b4bd1bbf7220882de39fe8ae6716aae4132da`. Requires Swift 6.2+ to opt out of
the NeMo normalization engine. In this release `traits: []` does **not** remove
TTS or diarization source; those opt-out traits appeared later on upstream main.

```sh
python3 benchmarks/apple-speech/fetch-model.py /tmp/tuck-apple-speech/models
swift build -c release --package-path benchmarks/apple-speech
/usr/bin/time -l benchmarks/apple-speech/.build/release/AppleSpeechBenchmark \
  /tmp/tuck-apple-speech/models /tmp/tuck-verification/en.wav ane 6
/usr/bin/time -l benchmarks/apple-speech/.build/release/AppleSpeechBenchmark \
  /tmp/tuck-apple-speech/models /tmp/tuck-verification/en.wav cpu 6
benchmarks/apple-speech/.build/release/AppleSpeechBenchmark \
  /tmp/tuck-apple-speech/models /tmp/tuck-verification/en.wav plan
```

Download is 483,105,645 bytes. `fetch-model.py` requests immutable Hugging Face
revision `7dd20fe6b1797d35f5e3307e8b1732d9a178edfe` and verifies upstream LFS SHA-256
or Git blob SHA-1 for every file. `model-manifest.json` records verified SHA-256
values for all files. No model downloader runs in the Swift executable:
`AsrModels.loadLocal` reads exactly the supplied directory.

The original encoder is named `.int8` in this SDK API but uses 6-bit palettized
weights according to upstream documentation. It is not numerically equivalent to
the sherpa-onnx int8 conversion. Each iteration starts a fresh decoder state.
Audio conversion is timed separately; inference receives 16 kHz mono Float32.
The first decode is cold in-process, subsequent iterations warm. Model-load time
includes any Core ML first-use preparation. OS file/cache state is uncontrolled.

The saved `ane-*` and `cpu-*` timing results use that original encoder. Production
uses the separately verified int8-linear encoder to address upstream issue 760;
do not apply the original performance numbers to it. To stage that selection for
explicit verification, add `--int8-v2` to `fetch-model.py`; it reuses companions
and writes a 632,169,729-byte manifest. The benchmark accepts `int8-v2` after its
repetition argument, including `plan 1 int8-v2` for anticipated placement. The
production helper and its final manifest are in `native/apple-speech`.

## Build workaround used for the recorded measurements

SwiftPM stalled fetching the declared NeMo xcframework even with `traits: []`.
The binary could be fetched and checksum-verified with curl, but SwiftPM did not
complete its own transfer. The recorded build used the exact pinned source with
only the unused binary target and its dependency reference removed from its
Swift 6.2 manifest. No inference source was changed. The patch is committed here:

```sh
git clone --branch v0.17.7 --depth 1 \
  https://github.com/FluidInference/FluidAudio.git /tmp/tuck-fluidaudio-source
git -C /tmp/tuck-fluidaudio-source rev-parse HEAD
git -C /tmp/tuck-fluidaudio-source apply \
  "$PWD/benchmarks/apple-speech/omit-unused-nemo.patch"
TUCK_FLUIDAUDIO_SOURCE=/tmp/tuck-fluidaudio-source swift build -c release \
  --package-path benchmarks/apple-speech \
  --cache-path /tmp/tuck-apple-speech/swift-cache \
  --config-path /tmp/tuck-apple-speech/swift-config \
  --security-path /tmp/tuck-apple-speech/swift-security
```

`plan` requires macOS 14.4+. It reports Core ML's **anticipated** device placement,
not a runtime hardware trace. `.cpuAndNeuralEngine` permits CPU fallback and
excludes GPU. CPU process time omits accelerator work and is not energy usage.
