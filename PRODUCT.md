# Product

<!-- impeccable:product-schema 1 -->

## Platform
web

The first release is a native desktop application using Tauri's system webview on macOS, Windows and Linux. Its React surface adapts to small screens. Mobile native capture is a separate platform phase: mobile operating systems restrict other-app call audio and desktop system audio capture cannot be assumed to work there.

## Stack
Selected with SGL: Tauri 2, a portable Rust domain/application core, native Rust adapters, TypeScript/React interface, sherpa-onnx CPU inference, and a local Swift/Core ML adapter for Apple Silicon. Direct code design requested. The initial Python prototype is retained only as a reference and benchmark baseline. Platform adapters can change without changing meeting rules.

## Users
People in meetings and sales calls who want private transcripts, useful notes, and immediate context without a meeting bot.

## Product Purpose
Capture microphone and system audio locally, produce speaker-attributed transcripts, summarize conversations, answer questions while a call is happening, and optionally send notes to a selected Notion database.

## Capabilities and Constraints
NVIDIA Parakeet is the requested speech engine. Efficiency, low resource use, and a simple workflow take priority. Hosted AI and Notion publishing require explicit configuration. ChatGPT sign-in must use the documented public authorization and Responses flow. Speaker clusters are provisional; names require user identification. No universal performance claims without measurements. Desktop audio needs OS permissions. Mobile system capture has platform restrictions.

The minimum comfortable target confirmed by SGL is a **16 GB laptop running an active video call**. Qualification must include the complete app, microphone/system audio processing, optional speaker labels and ordinary notes/chat interactions alongside the call. The current measurements on a 32 GiB Mac do not establish that target. Local LLM memory depends on the user's selected model and must be reported separately from the transcription pipeline.

## Evidence on Hand
Built from an empty repository. User referenced Muesli, Granola, Wispr Flow, Superwhisper and OpenAI's Sign in with ChatGPT cookbook. Competitive and technical findings belong in docs/research.md. Sample meetings must be labeled as examples.

## Product Principles
- Audio processing stays on the device.
- Disclose hosted transcript processing at the point of use.
- Save first; integrations must never jeopardize a meeting.
- Keep capture, inference and UI isolated and bounded.
- Never fabricate transcripts, speaker identities, status or benchmarks.
