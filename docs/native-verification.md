# Native production verification

Verified on October 23, 2026 on a Mac18,5, Apple M6, 32 GiB RAM, macOS 27.0.1 arm64. These checks used public audio and isolated temporary libraries. No user microphone, system audio, ChatGPT account, paid inference or Notion destination was accessed.

## Paged-library package

The current development bundle is **58.74 MiB (61,593,170 bytes)**, built from application source commit `43ddd455b8b2f1e7926dd108d253b6559b8c2d06`. Its main executable SHA-256 is `ec1d893b1453262cafee1c84d7697f2b78c3f4e15dba97f3cfe6d9d586765343`. Inventory confirms no Python runtime/files, speech weights or vocabulary qualification executable. The optional vocabulary experiment is developer-only and failed its false-insertion gate; [qualification](vocabulary-qualification.md) records that decision.

Final checks passed: **49 core tests, 72 native library tests, two desktop lifecycle tests, seven UI unit tests and 30 browser E2Es**, plus strict core/native Clippy and production builds. Two optional native fixture tests remain ignored in the ordinary suite. The actual native composition passed 32 large-library requests and a repeated 131-request safety check using real Parakeet/public speech. Independent Impeccable review approved selected detail **32/40**, library pages **31/40** and global tasks **32/40**; see [review](library-loading-ui-review.md).

The 1,000-meeting / 120,000-segment diagnostic fixture transferred 27,555 bytes at lean startup versus 76,224,483 bytes through the same binary's full compatibility bootstrap. Rust RSS sampled after each fresh process's response was about 24.1 versus 363.0 MiB. These are diagnostic observations excluding WebKit, speech models, local AI and a video call, not minimum-spec qualification. [Loading verification](library-loading-verification.md) contains the exact evidence and limits.

The final bundle was built again after the compact retry and failed-query recovery fixes. After the machine reconnected, the older process was quit and this exact package relaunched successfully. Native search with no results preserved the selected conversation; searching saved note contents found its actual meeting. A pending note survived immediate Cmd-W hide and reopening. The retained public clip loaded paused, played to its end and sought back while paused. File → Record a meeting opened setup without recording. No calendar/notification permission, microphone/system recording or external account was used. `benchmarks/native-workflow/final-package.json` records this package's fresh native acceptance separately from the earlier package. Real capture/account checks, signing and the16GB real-call target remain outstanding.

## Prior workflow package checkpoint

The current development bundle is **58.62 MiB (61,471,150 bytes)** at `src-tauri/target/release/bundle/macos/Tuck.app`. Its main executable SHA-256 is `b4af4ebc3fbf5d4f4bbbcb64baeee1f90fc2724f2649023e3f93b79286004932`. It contains the audio, calendar and Core ML speech helpers, the Symphonia MPL-2.0 notice, and zero Python files/interpreters. Speech weights remain separate explicit downloads. The earlier package sizes and test counts below are historical checkpoints.

Final automated results: **44 core tests**, **72 native library tests**, **two desktop lifecycle tests**, **six UI unit tests** and **23 browser E2Es** passed. Two opt-in native fixture tests were ignored in the ordinary suite; real public-audio integrations were run separately. Strict Clippy and the production frontend/native build passed. Independent Impeccable reviews retain writing 32/40, library/tasks 31/40, capture/settings 30/40, corrections 32/40, audio/recovery 31/40 and backup/restore 32/40.

The final package includes bounded streaming PCM WAV/MP3/FLAC/AAC-LC import, a native File → Record a meeting fallback, and the neutral manual-recap footer. Local AI requests two CPU threads, a 16,384-token context and a 4,096-token answer limit. Manual note autosaves retain periodic history checkpoints instead of duplicating the complete note on every edit; accepted replacements retain their exact prior text. ZIP metadata streams through a 128 MiB limited writer. These are configured bounds and regression-tested behavior, not a whole-app resource benchmark.

The final isolated workflow and safety runs passed 24 and 131 requests respectively, including genuine public speech through Parakeet and byte-identical restored audio. Codec tests passed, including public AAC-to-Parakeet correctness; AAC priming/padding remains in the retained normalized recording. Evidence is in `benchmarks/native-workflow/` and [audio import](native-audio-import.md).

After SGL unlocked the desktop, the older running process was quit and the final packaged bundle relaunched. Its neutral recap footer confirmed the refreshed frontend. In the actual native WKWebView, the public Kennedy clip loaded paused with a three-second duration, changed Play to Pause when started, advanced to its end, sought back to one second while paused, and resumed from that position. This verifies native protocol/media-control behavior; it does not establish audible speaker output, long-file streaming or hardware capture quality. File → Record a meeting opened Audio & models with a setup notice and did not start recording. Privacy displayed the backup/restore entry points. A note labeling the public fixture survived immediate Cmd-W hide and reopening the persistent app. No meeting was recorded, permission granted or external provider invoked. Package metadata is recorded in `benchmarks/native-workflow/final-package.json`.

## Meeting workflow checkpoint

The workflow increment adds tasks/organization, editable recaps, reviewable proposals/history, templates/recipes, bounded lexical library answers, calendar preparation, native quick controls/readiness and richer Notion publication. The corresponding app bundle built successfully at **56.53 MiB**, with no Python dependency. This is a newer bundle than the original native-window verification below. After SGL unlocked the Mac again, it was launched through computer-use automation. The existing library migrated without losing its verification note. A clearly labeled verification task was created, seen in the global inbox and completed. A manually edited recap survived immediate Cmd-W hide and reopening the persistent app. Native pixels and accessibility controls matched the reviewed workspace. No audio, calendar or provider action was invoked.

This native pass caught the remaining generic “AI recap” footer on manually written recaps; source now uses the neutral “Recap · review important details.” Its updated bundle is verified separately from the first checkpoint. A targeted synthetic Cmd-Shift-R through CUA did not visibly invoke the OS global hotkey, so that interaction is not claimed as hardware-qualified; the final bundle includes a native File-menu fallback.

- Core: **35 tests** passed, including migration/defaults, transaction rollback, correction-free evidence semantics, proposal conflicts, actual citations and Notion recovery hooks. Strict Clippy passed.
- Native: **55 library tests passed, one optional weights fixture ignored**, plus **two desktop lifecycle tests**. The adapter suite includes mock HTTP checks for Notion known-page reconciliation, incomplete replacement preservation, foreign-section isolation, read-only recovery and bounded pagination. No live Notion workspace was accessed.
- Interface: production build, **five unit tests and 17 browser E2Es** passed after the final workflow changes. Independent Impeccable review confirmed writing **32/40**, library/tasks **31/40** and capture/settings **30/40**; see [workflow review](workflow-ui-review.md).
- `scripts/verify-workflow.mjs` exercised **24 requests** through the actual Rust desktop composition in an isolated library: metadata/task changes, scoped evidence, stale-proposal rejection, revisions, saved writing, calendar metadata, non-recording preflight, native JSON export and complete-process restart persistence. Raw output and binary hash are in `benchmarks/native-workflow/workflow.json`.
- The synthetic public-PCM capture check passed again with real Parakeet: pause stopped WAV writes, resume continued genuine segments, stop drained evidence, immediate source failure recovered durably, and a subsequent recording started successfully. No actual microphone or system audio was accessed. The temporary fresh-library start also exercised model preparation/cache behavior; this correctness run is not a new latency benchmark.

The native model/pipeline and full 16 GB meeting qualification limits below remain in force. Device-calendar permission and five-second audio tests are implemented but were not invoked against personal calendars or hardware during these checks.

## Library safety checkpoint

The next increment adds schema 3 transcript corrections, original-evidence disclosure/reset, bounded local WAV playback, recovery into a separate linked meeting and portable ZIP backup/preview/merge restore. **66 native library tests plus two native lifecycle tests** passed, with one opt-in weights fixture ignored. The core suite additionally checks that shutdown waits for an in-progress archive write against both memory and SQLite adapters. Independent safety UI review scored **32/40, 31/40 and 32/40**; browser coverage reached **22 E2Es and six unit tests**.

`scripts/verify-library-safety.mjs` passed **131 requests** through the actual native composition using the public Kennedy fixture and real Parakeet. It verified immutable original text, effective corrections and scoped search, playback URL validation, recovery preserving the source audio, a portable backup with note history, preview fingerprint conflicts before writes, complete restore with byte-identical owned audio, safe restored preferences and idempotent repeat restore preserving intervening edits. Raw evidence and binary hash are in `benchmarks/native-workflow/library-safety.json`. The full history beyond the UI's newest-100 window is separately covered by core repository tests.

Playback unit tests cover bounded/suffix/HEAD requests and virtual WAV-header repair; the browser audio test exercises paused seeking against an HTTP Range fixture. Neither substitutes for an actual WKWebView media-protocol playback check. ZIP regressions cover duplicate/traversal/symlink/oversized input, original container-byte preservation and no overwrite of orphan audio. No personal recording, calendar or external provider was accessed.

## Runtime and package

The production Tauri host calls `crates/tuck-core` in-process. The Rust application composes SQLite, native capture, local speech, native credentials, official ChatGPT OAuth, AI providers and Notion adapters. Apple inference runs in a persistent local Swift/Core ML helper; CPU inference uses the official sherpa-onnx Rust binding. The former Python prototype is a reference under `engine/`, excluded from native setup and packaging.

`npm run native:setup` passed using the exact already verified FluidAudio checkout through `TUCK_FLUIDAUDIO_SOURCE=/tmp/tuck-fluidaudio-source`. The script verifies its commit and manifest-only patch. The default path clones that same pinned release; network access is required when the source is absent.

`npm run desktop:build -- --bundles app` produced:

`src-tauri/target/release/bundle/macos/Tuck.app`

The bundle is **54.07 MiB**, versus **124.93 MiB** for the earlier frozen Python prototype (about 57% smaller). Its only main executable is `tuck`; resources contain `tuck-speech`, `tuck-audio`, icon and attribution. An inventory found no Python interpreter, `.py` or `.pyc` files. The diagnostic `tuck-runtime` executable is feature-gated and excluded from the shipping app. Speech weights are separate explicit downloads, not included in these sizes.

Package size is not RAM usage. See [controlled Rust comparison](rust-evaluation.md) and [Apple model evaluation](apple-speech-evaluation.md) for measured memory, model conversion differences and hardware limitations. No end-to-end minimum-spec, long-meeting, energy or thermal qualification is established by this package check.

SGL confirmed the minimum comfortable target as a **16 GB laptop alongside an active video call**. Qualification must measure the full app and its helper/service costs during sustained microphone plus system-audio transcription, with speaker labels and normal notes/chat interactions. Record memory pressure/swap, CPU, transcription delay, queue failures and call responsiveness; measure any local LLM separately. The available 32 GiB Mac results remain development evidence rather than a pass for this target.

## Automated and integration checks

| Check | Result |
| --- | --- |
| Rust domain/application and memory/SQLite contracts | 21 tests passed |
| Native adapters, transport/security, capture and installer regressions | 43 passed; 1 opt-in real-weights test ignored |
| Strict native library Clippy | Passed with warnings denied |
| Rust native binaries compile | Passed |
| React production build and unit tests | Passed; 5 unit tests |
| Browser UI end-to-end checks | 8 passed again after the final recap-caption correction |
| CPU native runtime: empty startup, persistence/export, genuine Parakeet WAV import, clean shutdown | Passed |
| Core ML V2 native runtime: same workflow and genuine WAV import | Passed |
| Release runtime using the helper inside the final app bundle | Passed |
| Synthetic system source through the real recording/inference/application pipeline | Passed |
| Packaged native WKWebView launch, empty library, settings and model setup guidance | Passed |
| Native meeting rename and note persistence across window close/relaunch | Passed |
| Pending-note flush when closing immediately after editing | Passed |
| Three-minute public PCM recording with real Core ML V2 transcription and resource sampling | 37 segments, zero reported engine errors; see [sustained measurement](sustained-performance.md) |
| Prepared recording starts through two fresh model-library aliases | 1.400 s and 1.450 s, with real transcripts and clean shutdown |

The synthetic capture check (`scripts/verify-capture.mjs`) streams the public Kennedy PCM through a disposable replacement audio helper. It checks real Parakeet segments, pause stopping audio writes, resume, drain before completion, owned WAV output, immediate source failure after READY, durable interruption and a subsequent recording. It accesses neither real system audio nor a microphone. This establishes the native pipeline/lifecycle behavior; it does not validate ScreenCaptureKit permissions or routing.

The Apple helper separately passed bounded binary framing, fragmented/coalesced requests, short turns, shutdown, malformed input and missing-model failures. Its corrected int8V2 encoder passed the public upstream corruption reproduction and control audio. See [Apple evidence](apple-speech-evaluation.md). The original encoder's 32.3 ms timing is not attributed to the production V2 encoder.

Reproduce with already verified weights and public fixtures:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --release --features diagnostics --bin tuck-runtime
python3 scripts/verify-runtime.py --engine src-tauri/target/release/tuck-runtime \
  --models /absolute/models --fixture /absolute/public.wav
node scripts/verify-capture.mjs --engine src-tauri/target/release/tuck-runtime \
  --models /absolute/models --pcm /absolute/public-16k.f32
```

The Python command is an external test harness, not a production dependency. The synthetic process-source integration script currently targets macOS. Model/fixture downloads must be explicit; these checks do not automatically fetch weights.

## Native window verification

After the Mac was unlocked, computer-use automation opened the packaged `Tuck.app` successfully at `tauri://localhost`. The fresh native library was empty. Settings showed local AI, opt-in hosted features, and explicit model setup; creating a meeting correctly required speech-model setup before recording. No models were downloaded and no audio permissions were requested during this UI check.

A clearly named **Native app verification** draft was created, renamed and given a verification note through the native UI. Closing/relaunching restored both title and note. A second edit followed immediately by native-window close (the two automation actions completed in about 83 ms, within the normal 500 ms note debounce) also survived relaunch, exercising the pending-note close protection in WKWebView. The app was left open on that draft. This supplements the previous mocked close-flush checks; it does not establish every shutdown or OS-termination path.

The native appearance and accessibility tree matched the reviewed desktop design: labeled navigation and note controls, clear saved-local status, empty transcript guidance, and setup actions instead of a false recording state. Final confirmation corrected two recap captions: an empty recap no longer implies processing has occurred, and a generated recap no longer claims historical provider provenance from today's settings. The refreshed native bundle displayed the corrected text without layout changes. An independent [Impeccable continuation review](ui-native-confirmation.md) accepted the change and retained workspace 31/40 without increasing scores.

The native speech adapter now canonicalizes model directories consistently before preparation and inference. Two new temporary library aliases to the same verified physical model directory then started recording in 1.400 s and 1.450 s. These are observed prepared starts, not a guarantee against OS cache eviction; the earlier 82.97 s cold/preparation-like startup remains in the [measurement evidence](sustained-performance.md).

## Remaining acceptance checks

- Real macOS microphone/system routing and permissions, Windows WASAPI and Linux PulseAudio/PipeWire hardware remain unverified. Linux system capture requires `parec` and a monitor source.
- Real ChatGPT sign-in, account limits, hosted provider tools and Notion publication remain user-driven acceptance checks. Security/transport regressions use fake secrets, generated keys and deterministic payloads.
- Native mobile capture/inference, signing/notarization and public distribution are not complete. CI workflows are configured, not claimed to have run.

The independent UI review still records workspace 31/40, onboarding/library 31/40 and settings 30/40. See [UI assessment](ui-review-a.md); runtime verification does not inflate those design scores.
