# Third-party components and speech models

Tuck is an independent application. It is not affiliated with NVIDIA, OpenAI, Notion, or the projects below.

- **NVIDIA Parakeet TDT 0.6B v3**: NVIDIA Corporation. [Original model and CC BY 4.0 license](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3). Tuck uses an INT8 ONNX conversion published by [k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), downloaded on request. Tuck does not modify the published converted weights. The conversion/quantization changes the format and precision of the original NVIDIA model.
- **Sherpa-ONNX**: [k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), Apache 2.0. Provides the recognition/VAD/speaker runtime and converted model release assets.
- **FluidAudio**: [FluidInference/FluidAudio](https://github.com/FluidInference/FluidAudio), Apache 2.0, pinned to v0.17.7 source revision `503b4bd1bbf7220882de39fe8ae6716aae4132da`. The Apple helper uses local loading through Core ML. A reviewed manifest-only patch omits an unused NeMo binary; inference source is unchanged.
- **Parakeet v3 Core ML conversion**: [FluidInference/parakeet-tdt-0.6b-v3-coreml](https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml), derived from NVIDIA Parakeet TDT 0.6B v3, CC BY 4.0. Conversion and quantization change format and precision. Exact source revision, file hashes and selected encoder are recorded in `native/apple-speech/models.json`; model weights are installed separately by explicit action. NVIDIA Corporation retains attribution for the original model.
- **CPAL**: [RustAudio/cpal](https://github.com/RustAudio/cpal), Apache 2.0. Native microphone capture and Windows WASAPI loopback.
- **Hound**: [ruuda/hound](https://github.com/ruuda/hound), Apache 2.0. PCM WAV evidence and imports.
- **Symphonia 0.6.1**: [pdeljanov/Symphonia](https://github.com/pdeljanov/Symphonia), Mozilla Public License 2.0. Unmodified Rust codec/demuxer components for bounded local file import; enabled AAC, FLAC, MP3, PCM, ISO MP4, RIFF/WAV and metadata components are pinned in `src-tauri/Cargo.lock`. Covered component source is available from the exact [0.6.1 crate release](https://crates.io/crates/symphonia/0.6.1) and its versioned dependencies; license text is supplied in `licenses/Symphonia-MPL-2.0.txt` in the native bundle and `assets/licenses/` in this source tree.
- **Silero VAD**: [snakers4/silero-vad](https://github.com/snakers4/silero-vad), MIT. Speech activity model downloaded as part of local transcription setup.
- **3D-Speaker / CAMPPlus**: [modelscope/3D-Speaker](https://github.com/modelscope/3D-Speaker). Optional English VoxCeleb speaker embedding model, distributed by Sherpa-ONNX. See the upstream project and model's terms. It produces anonymous voice embeddings; Tuck uses it for provisional meeting-scoped labels.
- **ONNX Runtime**: [microsoft/onnxruntime](https://github.com/microsoft/onnxruntime), MIT.
- **Tauri**: [tauri-apps/tauri](https://github.com/tauri-apps/tauri), MIT / Apache 2.0.
- **React**: [facebook/react](https://github.com/facebook/react), MIT.
- **Lucide**: [lucide-icons/lucide](https://github.com/lucide-icons/lucide), ISC.
- **Python reference implementation only**: Python (PSF), NumPy, python-soxr/libsoxr, PortAudio, sounddevice, SoundCard, HTTPX, PyJWT, cryptography, keyring and the former PyInstaller packaging are retained for tests and historical comparison under `engine/`. They are not bundled or launched by the production native build. See dependency metadata for their licenses.

Speech weights are separate explicit downloads, not part of the source tree. Their exact download URLs, sizes and SHA-256 hashes are pinned in the model-manager adapter. Further licensing/notice checks should accompany a public release on each target platform.
