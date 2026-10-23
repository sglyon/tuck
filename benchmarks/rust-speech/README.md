# Same-model speech benchmark

This isolated executable reuses the verified Parakeet v3 int8 model and public Kennedy test fixture. It never downloads a model, records audio, or changes the application. The Rust crate pins the official sherpa-onnx API to 1.13.8; Cargo.lock pins its dependency graph.

Run from the repository root:

```sh
engine/.venv/bin/python benchmarks/rust-speech/prepare_pcm.py /tmp/tuck-verification/en.wav /tmp/tuck-verification/en-16k.f32
cargo build --release --locked --manifest-path benchmarks/rust-speech/Cargo.toml
/usr/bin/time -l benchmarks/rust-speech/target/release/tuck-speech-benchmark /tmp/tuck-verification/models/parakeet-v3-int8 /tmp/tuck-verification/en-16k.f32 2
/usr/bin/time -l engine/.venv/bin/python benchmarks/rust-speech/python_baseline.py /tmp/tuck-verification/models/parakeet-v3-int8 /tmp/tuck-verification/en-16k.f32 2
```

The default Rust build downloads the publisher's native static library package at build time. The Python comparison uses the existing engine environment. To isolate the language boundary, link Rust against the exact native libraries from that Python installation:

```sh
SHERPA_ONNX_LIB_DIR="$PWD/engine/.venv/lib/python3.12/site-packages/sherpa_onnx/lib" cargo build --release --locked --manifest-path benchmarks/rust-speech/Cargo.toml --no-default-features --features native-shared
/usr/bin/time -l /usr/bin/env DYLD_LIBRARY_PATH="$PWD/engine/.venv/lib/python3.12/site-packages/sherpa_onnx/lib" benchmarks/rust-speech/target/release/tuck-speech-benchmark /tmp/tuck-verification/models/parakeet-v3-int8 /tmp/tuck-verification/en-16k.f32 2
```

Save the static executable before rebuilding shared if comparing both. Build scripts can copy dylibs into the target directory; the explicit `DYLD_LIBRARY_PATH` makes the intended comparison clear. Apply the variable through `env` after `time`, because macOS can strip loader variables when launching the protected system `time` executable. This shared comparison needs the wheel's files for this experiment, but no Python interpreter participates in the Rust inference process.

Both harnesses measure model loading, first decode, five warm decodes, eight sequential copies of the short clip, and all eight copies concatenated into a 30.761-second input. Set `TUCK_BENCH_SHORT=1` to skip both longer workloads for a short-clip peak-memory measurement. Times exclude process startup, PCM preparation, model checksums, VAD, speaker extraction, capture, IPC, and the UI. The cold measurement is a fresh recognizer in a fresh process with filesystem caches potentially warm; it does not mean a cold disk cache. Run benchmarks without other compilation/inference workloads.

Results are synthetic throughput checks, not accuracy evaluation or long-meeting memory guarantees. The benchmark asserts the expected text and identical repeated short transcripts. A provider string records what was requested; it is not evidence that the requested accelerator executed the graph.
