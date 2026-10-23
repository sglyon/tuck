# Tuck

A private desktop workspace for your meetings. Capture your microphone and computer audio without a meeting bot, transcribe locally with NVIDIA Parakeet, and ask an assistant about the conversation while it happens.

**Development build.** macOS, Windows and Linux adapters are implemented; this checkout is being verified on Apple Silicon macOS. Mobile has a responsive interface and reusable workflow, but no native mobile capture/inference runtime yet.

## Run locally

Install Node.js 22+, Rust and [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/). Building the Apple helper requires Swift 6.2+ / current Xcode. On Linux, install ALSA development libraries and `parec` (usually `pulseaudio-utils`) with a functioning PulseAudio/PipeWire monitor; Windows uses WASAPI loopback. End users do not need Rust, Swift or Python.

```sh
npm install
npm run native:setup
npm run desktop
```

Open Settings → Private transcription and download the speech model before recording. The download is explicit; Tuck never downloads speech weights or starts recording on launch. On macOS, allow Microphone and Screen & System Audio Recording when prompted. Use headphones for clean separation of your voice and the other side of the call.

`npm run dev` opens an interface-only browser preview at `http://127.0.0.1:1420`. Open the labeled example to explore it; browser preview cannot record system audio or use native credentials.

## AI that fits your privacy needs

- **On this device:** use [Ollama](https://ollama.com), install a chat model, then refresh the models in Tuck's settings. Tuck only connects to loopback addresses in this mode. AI is optional; local transcription does not require a chat model.
- **ChatGPT:** Continue with ChatGPT in Settings. Eligible accounts can authorize use of their plan. Tuck uses the official public OAuth/Responses integration, verifies identity, and keeps tokens in the OS credential store. Model choices come from the account's catalog.
- **OpenAI API:** save your own API key in the OS credential store and choose a supported text model.

Hosted processing requires an explicit setting. It sends transcript context, notes, and chat history as text; audio remains local. Web lookup is a separate opt-in for hosted chat. It is subject to the selected model and account's tool permissions. Tuck links returned sources. The local assistant has no internet-search fallback.

Automatic recap generation is opt-in. Personal notes are never replaced by an AI recap. Speaker labels are provisional until you name them; the optional speaker model clusters voices within a meeting and does not identify people by name.

## A meeting day

Use the tray menu or **⌘/Ctrl ⇧ R** to start a meeting. The shortcut checks setup first; opening or importing a calendar event never starts recording. Connect the device calendar on macOS or import a local `.ics` snapshot to prepare a title and attendees. Audio settings include device selection and an explicit five-second sound check that does not save or transcribe audio.

Recaps and personal notes are separately editable. Saved templates shape recaps; recipes run repeatable questions or prepare a proposed rewrite for review. Proposals preserve intervening edits. History keeps periodic checkpoints of manual edits and the text before accepted replacements, with prior versions available to restore. Add actions with owners and due dates, then use the task inbox to follow through across meetings.

Folders, tags and people narrow the library. Search and library questions retrieve bounded matching excerpts and link back to their actual meetings. This is lexical retrieval, so specific names and phrases work best. It does not run an embedding model in the background. Closing the window saves edits and leaves Tuck available in the tray; Quit saves and shuts it down.

The library loads in small pages; transcripts load when you open a conversation. Search, filters and the task inbox still cover meetings beyond the current page. [Synthetic large-library verification](docs/library-loading-verification.md) records the transfer and native memory observations separately from real-call qualification.

Retained recordings can be played and sought from Transcript without loading the whole file into the interface. Transcript corrections keep the original recognized words available and guard against stale edits. Recover saved audio creates a separate linked meeting; it keeps the source meeting and recording intact.

Privacy settings can save a portable ZIP backup of the library, saved writing and full note history, with optional retained audio. Restore previews the archive and imports only missing meetings. Existing meetings are skipped, credentials are excluded, and restore never enables hosted processing or automatic publishing. Backup files contain private content and are not encrypted by Tuck.

## Send notes to Notion

Create a Notion internal integration, grant it access to the destination database, and save its token in Settings. Find a shared destination in the picker, or paste its database URL/data source ID. Optional column mappings send the meeting date, folder, tags, people labels or local meeting ID into compatible existing columns. Tuck does not create columns or infer Notion user IDs from names.

Publishing sends a **snapshot of the recap, personal notes and action items**, not audio or the full transcript. Automatic publishing after a meeting is opt-in. Update notes refreshes the same page: a replacement Tuck section is verified before the prior section is retired; content outside Tuck's identified sections stays untouched. Old flat exports receive a managed section while their original content is preserved.

If a create request has an uncertain result, Tuck pauses new uploads to avoid duplicates. Check Notion and link the existing page; Tuck verifies its destination and meeting marker without writing to it. An interrupted update to a known page can be reconciled without creating another page. Export Markdown if you need a portable full transcript.

## Data and architecture

The native host uses Tauri's system webview, a portable Rust application core and native adapters. Apple Silicon uses a small local Swift/Core ML speech helper; the portable CPU path uses the official sherpa-onnx Rust binding. **The shipping app does not launch or bundle Python.** The earlier prototype in `engine/` remains a regression reference and benchmark baseline. One cohesive Meeting Workspace owns lifecycle, evidence, notes and chat. Core-owned ports isolate SQLite, capture, speech/AI providers, credentials and publishing. No cloud backend, event bus or dependency injection framework is required.

SQLite uses WAL and migrations. Credentials are held by the OS keyring, never SQLite or browser storage. Transcript/notes and retained WAV audio are local files, **not encrypted by Tuck**; use device encryption and access controls appropriate to your needs. Deleting a meeting removes its local records and owned audio, not a copy already published to Notion. See [architecture](docs/architecture.md), [runtime contracts](docs/contracts.md), and [research](docs/research.md).

Local libraries use Tauri's application data directory (`com.sglyon.tuck`): macOS Application Support, Windows AppData, and Linux XDG data. The same native composition can run against an explicit isolated directory for diagnostics:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --features diagnostics --bin tuck-runtime -- --data-dir /tmp/tuck-library
```

## Verify and package

```sh
npm run build
npm test
npm run engine:test
npm run test:e2e
npm run native:setup
npm run desktop:build
```

Cargo builds the Rust backend; `native:setup` builds and stages platform helpers. Speech weights remain a separate explicit download. Apple model installation includes device optimization before it is marked ready. Build on each target platform. Release signing/notarization and real meeting capture need platform validation before distribution. CI is configured for Rust core/native adapter checks and reference tests on macOS, Windows and Linux; this does not mean those jobs have run. A separate manual “Build desktop packages” workflow produces unsigned platform packages. CI does not simulate OS permission prompts or real meeting audio.

## Present limits

- The minimum comfortable target is a **16 GB laptop alongside an active video call**. This is the product requirement; the existing 32 GiB Mac measurements do not yet qualify it. The [three-minute native measurement](docs/sustained-performance.md) records a bounded public-audio baseline. Local chat-model memory is additional and depends on the model selected.
- Desktop-first; no native mobile capture, automatic meeting-app detection or system-wide dictation yet. Device-calendar integration is read-only; direct Google/Outlook OAuth is not configured. ICS import is a snapshot and does not expand recurrence rules.
- Apple Silicon uses an explicitly prepared Parakeet Core ML conversion with CPU/Neural Engine enabled. Other desktops use a bounded-thread quantized CPU baseline. CUDA/DirectML are candidates for a separately validated adapter, not enabled merely by naming a provider. [Measured comparisons](docs/rust-evaluation.md) and [Apple evaluation](docs/apple-speech-evaluation.md) record exact hardware, models and caveats. No minimum-spec or long-meeting resource qualification is claimed yet.
- Speaker clustering is provisional per speech turn. Overlapping voices and echo from loudspeakers can reduce quality; no acoustic echo cancellation is implemented yet.
- Audio import accepts PCM WAV, MP3, FLAC and AAC-LC (`.m4a`, `.mp4`, `.aac`, `.adts`), with one or two channels at 8–192 kHz, up to 2 GiB and 12 hours. Tuck streams conversion to local mono PCM without changing the original. AAC priming/padding is retained; protected audio and ALAC are not supported. See [audio import](docs/native-audio-import.md).
- Model downloads and hosted features need a connection. Ordinary local transcription works offline after setup.
- Hosted/realtime persistence will need account ownership, authorization and offline conflict decisions as well as a new repository adapter. Those are intentionally absent from this local release.
