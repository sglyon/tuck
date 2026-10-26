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

## Live meeting agents

In **Settings → Intelligence → Meeting agents**, edit the Quiet meeting helper and Sales coach or add your own monitoring goals. Choose up to three agents in the assistant pane and turn them on for that call. Agents offer transcript-linked questions, insights and follow-ups; Save follow-up explicitly adds a task for review. They cannot send messages, browse or act on your behalf.

Agents share your selected AI provider. Automatic context contains a bounded recent transcript window, the selected instructions and a few earlier tips; it excludes handwritten notes, meeting metadata and manual chat. Monitoring pauses with recording and stops when the call ends. It starts off again after restart or an AI destination/account change. Quiet pacing, bounded requests and a three-open-tip limit keep help unobtrusive. See [behavior and limits](docs/live-coach-contract.md) and [verification](docs/native-verification.md).

## Dictation anywhere

Click a text field in another app, then press **Cmd/Ctrl + Shift + Space** (the default shortcut) to speak. Press it again to finish. Tuck stays in the background, saves a local voice note and copies the recognized words. The tray menu and Settings → Audio & models provide alternatives when the shortcut is unavailable.

On macOS, Tuck can send a paste to the same confirmed editable field when you allow Accessibility access. Changing apps or fields, protected inputs and uncertain targets leave the text on the clipboard for manual paste. Windows and Linux use the clipboard/manual-paste path. Accessibility is optional for recording and copying; Tuck only opens its settings when you ask. “Paste sent” confirms dispatch, not that another app accepted the text.

Choose **Dictate a note** for an editable note inside Tuck. **Copy text** uses your edits; **Original speech → Copy original speech** keeps the full recognized words available separately. Global dictation copies original speech rather than later edits. **Save without inserting** keeps the note without copying or pasting.

Both workflows reuse the installed local Parakeet engine and microphone, with no second model, AI rewrite, network request or automatic Notion publication. After initial model setup, dictation works offline. This is Tuck’s own shortcut; it does not replace Apple’s dictation engine or take over Fn/Globe. See the [dictation contract](docs/dictation-contract.md).

## A meeting day

Use the tray menu or **⌘/Ctrl ⇧ R** to start a meeting. The shortcut checks setup first; opening or importing a calendar event never starts recording. Connect the device calendar on macOS or import a local `.ics` snapshot. Next meeting opens preparation notes with the title and attendees; returning to the same occurrence reuses your existing notes. Optional five-minute reminders appear in Tuck, with system notifications where available while the app is running. Reminders stay quiet during recording, and never join a call or start capture. Calendar snapshots remain on this device and are excluded from portable backups. Audio settings include device selection and an explicit five-second sound check that does not save or transcribe audio.

Recaps and personal notes are separately editable. Saved templates shape recaps; recipes run repeatable questions or prepare a proposed rewrite for review. Proposals preserve intervening edits. History keeps periodic checkpoints of manual edits and the text before accepted replacements, with prior versions available to restore. Add actions with owners and due dates, then use the task inbox to follow through across meetings.

Folders, tags and people narrow the library. Search and library questions retrieve bounded matching excerpts and link back to their actual meetings. This is lexical retrieval, so specific names and phrases work best. It does not run an embedding model in the background. Closing the window saves edits and leaves Tuck available in the tray; Quit saves and shuts it down.

The library loads in small pages; transcripts load when you open a conversation. Search, filters and the task inbox still cover meetings beyond the current page. [Synthetic large-library verification](docs/library-loading-verification.md) records the transfer and native memory observations separately from real-call qualification.

Retained recordings can be played and sought from Transcript without loading the whole file into the interface. Transcript corrections keep the original recognized words available and guard against stale edits. Recover saved audio creates a separate linked meeting; it keeps the source meeting and recording intact.

Privacy settings can save a portable ZIP backup of the library, saved writing and full note history, with optional retained audio. Restore previews the archive and imports only missing meetings. Existing meetings are skipped, credentials are excluded, and restore never enables hosted processing or automatic publishing. Backup files contain private content and are not encrypted by Tuck.

## Send notes to Notion

Choose **Connect with Notion**, approve the pages Tuck may access, then choose **Create my meeting database** and select a shared parent page. Tuck creates its standard **Tuck meetings** database, arranges the Recent meetings table and maps meeting details automatically. You can also use an existing database. Customers do not need to create integrations or copy tokens. The Tuck operator registers one public Notion connection and configures the deployed Cloudflare authorization relay; [operator setup](docs/notion-connection.md) records the deployed configuration. Existing internal tokens remain under Advanced connection. Find a shared destination in the picker, or paste its database URL/data source ID. Optional column mappings send the meeting date, folder, tags, people labels or local meeting ID into compatible existing columns. Tuck never alters another database’s columns or infers Notion user IDs from names. Its standard template includes date, folder, tags, people, meeting ID and creation time; each published page contains your selected recap, notes and actions. Interrupted setup checks for the existing template instead of blindly creating another. See [template behavior](docs/notion-template-native.md).

Choose which **recap, personal notes and action items** to publish. Audio, chat and the full transcript are excluded. Automatic publishing when a meeting finishes is opt-in; it does not require an AI recap and does not apply to voice notes. Local notes remain available after publication. The Cloudflare relay handles authorization only; selected meeting text goes directly to Notion, where Notion stores the published copy. Update notes refreshes the same page: a replacement Tuck section is verified before the prior section is retired; content outside Tuck's identified sections stays untouched. Old flat exports receive a managed section while their original content is preserved.

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
- Desktop-first; no native mobile capture or automatic meeting-app detection. Global dictation is implemented; automatic paste is currently macOS-only and requires a confirmed original field. Device-calendar integration is read-only; direct Google/Outlook OAuth is not configured. ICS import is a snapshot and does not expand recurrence rules.
- Apple Silicon uses an explicitly prepared Parakeet Core ML conversion with CPU/Neural Engine enabled. Other desktops use a bounded-thread quantized CPU baseline. CUDA/DirectML are candidates for a separately validated adapter, not enabled merely by naming a provider. [Measured comparisons](docs/rust-evaluation.md) and [Apple evaluation](docs/apple-speech-evaluation.md) record exact hardware, models and caveats. No minimum-spec or long-meeting resource qualification is claimed yet.
- Speaker clustering is provisional per speech turn. Overlapping voices and echo from loudspeakers can reduce quality; no acoustic echo cancellation is implemented yet.
- Audio import accepts PCM WAV, MP3, FLAC and AAC-LC (`.m4a`, `.mp4`, `.aac`, `.adts`), with one or two channels at 8–192 kHz, up to 2 GiB and 12 hours. Tuck streams conversion to local mono PCM without changing the original. AAC priming/padding is retained; protected audio and ALAC are not supported. See [audio import](docs/native-audio-import.md).
- Model downloads and hosted features need a connection. Ordinary local transcription works offline after setup.
- Hosted/realtime persistence will need account ownership, authorization and offline conflict decisions as well as a new repository adapter. Those are intentionally absent from this local release.

## Signed macOS releases

The root Makefile builds Apple Silicon or Intel packages for the current Mac. It requires the tools above and a valid Developer ID Application certificate with its private key in Keychain. All generated release files stay under the ignored `src-tauri/target/` directory.

One-time setup: create an app-specific password at https://account.apple.com, then run this in an interactive terminal:

```sh
make auth APPLE_ID=your-apple-developer-email
```

Enter the app-specific password at the secure prompt. `notarytool` validates the account and stores credentials under the `tuck-notary` Keychain profile. Never put the password in the Makefile, shell arguments, or repository. Existing profiles can be selected with `NOTARY_PROFILE=profile-name`.

Quit Tuck and run:

```sh
make release
```

This checks notarization credentials, installs locked npm dependencies, builds native helpers, signs all helpers and the release app with hardened runtime and a timestamp, submits the app to Apple, staples its ticket, builds and signs a DMG without Finder automation, notarizes and staples the DMG, verifies signatures/tickets/Gatekeeper, and copies the app to `/Applications`. The final DMG is `src-tauri/target/release/bundle/distribution/Tuck_<version>_<architecture>.dmg`. Speech models are downloaded separately in the app.

The default signing identity is the first Lyon Cubs Developer ID Application certificate found by `security find-identity`; override with `SIGNING_IDENTITY=<certificate-SHA1>` when needed. `APPLE_TEAM_ID`, `NOTARY_PROFILE`, and `INSTALL_DIR` can also be overridden. Signing uses the local private key without exporting it.

To build without submitting to Apple, use `make signed-app dmg`. To finish the current signed build after configuring credentials, use `make notarize-app dmg notarize-dmg verify install`. Each submission prints its ID; if a wait times out, inspect or wait for that ID with `xcrun notarytool info <id> --keychain-profile tuck-notary` or `xcrun notarytool wait <id> --keychain-profile tuck-notary`. For a rejected submission, retrieve Apple's diagnostics with `xcrun notarytool log <id> --keychain-profile tuck-notary`. After acceptance, staple the relevant app or DMG with `xcrun stapler staple <path>` before continuing subsequent targets.

`make install` refuses to replace a running Tuck app. It does not force-quit the app or discard unsaved work. Installation into `/Applications` may require local administrator permissions.

The selected royal-blue app icon source is `assets/icon.png`; `make icons` regenerates platform icons. Signed builds run this target automatically. `assets/icon.svg` retains the earlier waveform artwork as a legacy source.

The in-app brand uses the same `assets/icon.png` source as the native app icons. Installation refreshes the app’s Launch Services registration; run `make refresh-icons` to refresh it separately.

Configure global dictation in Settings → Audio & models → Dictation anywhere: turn it on or off, choose a shortcut, and choose automatic paste or clipboard-only. Preferences are saved on this device; changing them never starts recording.

Library and Tasks are the main navigation destinations. Use New for a meeting, voice note or audio import. Search library (Cmd/Ctrl-K) offers local keyword search and Ask Tuck across saved items. All/Starred and list filters affect saved-item browsing; search and tasks have independent filters.

To change the global dictation shortcut, open Settings → Audio & models, click **Record shortcut**, and press the desired key chord. Function keys such as F20 work without modifiers. Escape or leaving the recorder cancels capture. Click **Save dictation settings** to apply it.
