# Local speech verification

> Historical Python prototype measurements. The production runtime is now Rust/native; see [native verification](native-verification.md), [Rust comparison](rust-evaluation.md), and [Apple evaluation](apple-speech-evaluation.md). These figures do not describe the current Core ML pipeline.

Verified on 2026-10-23 on this Apple Silicon macOS host. These are measurements from one small public test recording, not product-wide performance guarantees.

The installed runtime is Python 3.12.7, sherpa-onnx 1.13.8, explicit sherpa-onnx-core 1.13.8, NumPy, and libsoxr. The explicit core dependency works around the upstream wheel/sdist dependency omission documented in [sherpa-onnx issue 3689](https://github.com/k2-fsa/sherpa-onnx/issues/3689). No PyTorch or NeMo runtime is installed.

## Genuine inference test

`engine/.venv/bin/python scripts/verify-speech.py --data-dir /tmp/tuck-verification`

The command imports the publisher's checksum-verified 3.845-second, 24 kHz English WAV, streams resampling to 16 kHz, segments it with Silero VAD, transcribes it with NVIDIA Parakeet TDT v3 int8 on two CPU threads, and runs the optional speaker embedding extractor. It asserts meaningful expected text, bounded timestamps and automatic model unloading after import.

Last run: 1.434 seconds including cold model loading, checksum verification, resampling and inference. Peak process RSS was 1,955,037,184 bytes (about 1.82 GiB). Models are released after import or recording stop; this peak is not idle app memory. The produced segment spans 0.070–3.845 seconds and contains the expected Kennedy quotation beginning “Ask not what your country can do for you”. The speaker extractor was used and assigned the provisional ID `speaker-1`.

The model weights and test fixture remain at `/tmp/tuck-verification/models/` and `/tmp/tuck-verification/en.wav` for verification. They are not committed, and app startup never copies or downloads them. On a fresh host, add `--install --download-fixture` explicitly to download the pinned model packages and fixture. This uses approximately 518 MB of network transfer.

Asset hashes are pinned in `engine/tuck_engine/adapters/model_manager.py`: Parakeet archive and Silero hashes from the publisher's release asset metadata, and the English CAMPPlus speaker hash from the publisher's checksum.txt. The test WAV hash is pinned in `scripts/verify-speech.py`. Extraction copies only expected regular model files and never extracts arbitrary archive paths or links.

## Automated adapter checks

`PYTHONPATH=engine engine/.venv/bin/python -m unittest discover -s engine/tests -p test_capture.py -v`

19 tests pass. They cover source startup failure and retry, pausing hardware and excluding paused samples, draining the inference queue before completion, preservation until application retention finalization, preservation after failure, reader failure callbacks during startup/resume, overwrite prevention, WAV import validation and final partial chunks, streaming 48 kHz resampling, traversal rejection, no automatic downloads, bounded speaker clustering, and safe model installation/extraction failure.

## Native helper

The ScreenCaptureKit helper compiled successfully to `src-tauri/binaries/tuck-audio` (approximately 93 KB). `npm run engine:setup` compiles it using a project-local Swift module cache. It emits PCM audio only; no screen frames are requested or retained.

## Unverified and limited

No microphone or system audio from the user was recorded during these checks. macOS permission prompts, actual meeting routing, Windows WASAPI, and Linux PulseAudio/PipeWire monitors still require device tests. The helper's successful compilation does not establish successful hardware capture.

The local speech pipeline uses bounded chunks and a bounded queue, preserving incremental WAV recordings before inference. If inference falls behind or fails, capture stops and reports recovery instructions. Speaker clustering is per-turn and provisional: it does not separate overlapping voices or recognize named people. Microphone speech is attributed to `you` by source, so in-person multi-speaker meetings need a future microphone diarization mode. There is no acoustic echo cancellation in this version; headphones avoid microphone pickup of speaker output.

Imports support mono/stereo 16-bit PCM WAV at 8–192 kHz. Other audio codecs require conversion. The optional speaker model is English-focused. Desktop Python inference is not yet a mobile runtime.
