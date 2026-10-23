# Meeting workflow progress

This extends the competitive research in [research.md](research.md). Implemented behavior, mock/integration verification and real platform qualification are different kinds of evidence. See [workflow-core.md](workflow-core.md), [workflow-ui-review.md](workflow-ui-review.md) and [native-verification.md](native-verification.md) for the relevant checks.

## Implemented in the workflow wave

| Need | Implementation | Limit |
| --- | --- | --- |
| Quick capture | Persistent tray, explicit record shortcut, setup gating, save before hide/quit | No automatic detection of meeting apps or ambient recording |
| Meeting context | Read-only macOS EventKit connection, persistent next meeting, atomic occurrence preparation, optional five-minute reminders, portable ICS import | Device calendar and native notification delivery need platform acceptance; reminders never record or join; ICS is a snapshot without recurrence expansion; no direct Google/Outlook OAuth |
| Capture confidence | Device selection, preflight readiness, explicit five-second sound check, silence warnings | Real microphones/system capture and permission behavior still need hardware qualification |
| Human-owned notes | Separate editable recap/notes, reviewable AI proposals, conflict guards, periodic manual checkpoints and exact pre-replacement versions | AI output still needs review; transcript corrections preserve the original speech separately |
| Reusable work | Meaningful built-in/custom recap templates and saved recipes | Recipes produce reviewable text; they do not send external messages |
| Live assistance | Configurable per-call transcript monitors, quiet and active defaults, cited suggestions and explicit Save/Dismiss | Uses selected local/authorized hosted model; no autonomous tools, messages, web research or external actions |
| Follow-through | Per-meeting actions with owner/date/completion and a task inbox | No external task-system synchronization or automatic commitment creation |
| Organization | Folders, tags and people scopes | People labels do not establish speaker identity |
| Library recall | Bounded lexical evidence retrieval, cross-meeting questions, actual source links | No semantic index, persistent library conversation history or unbounded transcript context |
| Growing library | Paged headers, global scopes/counts/tasks and on-demand meeting details | Selected meetings still load complete transcripts; literal search work grows with stored evidence |
| Notion | Guided standard database creation, automatic mappings, shared-destination discovery, known-page updates, read-only uncertain-page recovery | Public OAuth adapter and Cloudflare relay deployed; live sign-in and destination discovery passed. Live template creation, completed view, publication and same-page update passed using the labeled public verification note. Advanced internal tokens remain supported; unknown create outcomes never trigger a blind duplicate upload |

Notion updates preserve content outside marker-verified Tuck sections. A complete replacement is read back before an earlier section is retired. Reconciliation compares actual content rather than trusting a revision label. Older flat exports stay in place; the first update adds a managed section. Mock HTTP tests cover ambiguity, actual-content reuse, partial replacement, foreign sections, bounded cursors and read-only recovery. These tests do not claim a live Notion account was exercised.

## Next product work

The subsequent library-safety increment implements retained-audio playback, guarded transcript corrections/reset with original speech preserved, linked recovery workspaces and portable ZIP backup/preview/merge restore. Its UI review meets the floor at 32/40, 31/40 and 32/40. An isolated native integration check made 131 requests with real Parakeet/public speech and verified exact audio bytes after restore, stale correction/fingerprint rejection and preservation of current edits on repeat restore. See `benchmarks/native-workflow/library-safety.json`, [safety contract](library-safety-contract.md) and [UI review](safety-ui-review.md).

Meeting-day preparation and optional reminders are implemented in the next slice. Reopening an occurrence preserves its original workspace; reminder claims and dismissal survive restart. Bounded cached schedules and delivery state stay outside portable backups, and restore cannot enable reminders. Independent Impeccable review scored the changed interface33/40. Generated-calendar native integration verifies preparation, restart, dismissal and backup privacy without accessing personal calendars or OS permissions. See [meeting-day contract](meeting-day-contract.md), [native behavior](meeting-day-native.md) and [UI review](meeting-day-ui-review.md). Actual OS notification presentation remains unqualified.

Broader file import is also implemented: PCM WAV, MP3, FLAC and AAC-LC stream through bounded local conversion without modifying the selected source. Codec fixtures and a public AAC-to-Parakeet integration passed. AAC edit-list trimming remains unsupported, so priming/padding is retained. See [audio import](native-audio-import.md).

1. Custom vocabulary with a validated recognition path. The [acoustic qualification](vocabulary-qualification.md) corrected names but also inserted one into ordinary speech, so it remains a developer prototype. [Backend evaluation](vocabulary-evaluation.md) records the remaining requirements.
2. Automatic meeting-app detection, persistent voice identity and better overlap/echo handling, each with realistic privacy and accuracy checks.
3. Native mobile capture and device continuity; hosted synchronization/sharing require account ownership, offline conflicts and retention decisions.
4. Cross-application dictation and snippets. In-app microphone-only voice notes are now implemented with editable text, original recognized speech and explicit copy; they reuse Parakeet and skip meeting automation.

## Connection and voice-note increment

In-app dictation reuses the meeting lifecycle, existing speech model, recording archive and SQLite records with a defaulted workspace kind. No new model or table is required. Notion publishing adds three explicit content choices; excluded personal text is removed before crossing the publication port, and raw evidence/chat/audio never enters its payload. The public connection uses a deployed Cloudflare Worker with one expiring authorization transaction per Durable Object; it stores no meeting notes or saved credentials. Operator registration and live sign-in are complete. Independent reviews approve the changed interface at32/40 and33/40; source speech remains available even when a user types notes while recording. See [dictation contract](dictation-contract.md), [Notion connection](notion-connection.md) and [UI review](connection-dictation-ui-review-a.md).

The subsequent template setup creates an explicitly chosen destination with seven consistent columns and a Recent meetings view. It atomically saves mappings without enabling publishing. The device-local setup journal survives interruption, stays outside backups, and makes uncertain retries read-only. Independent Impeccable assessments scored31/40 and32/40, with material warning/schema issues corrected. See [template contract and checks](notion-template-native.md), [UI evidence](notion-template-ui.md), and [review](notion-template-ui-review-a.md).

## Configurable live-agent increment

Settings → Intelligence contains two editable starting agents and a small custom-agent editor. Each call chooses up to three agents and explicitly turns them on. A single bounded model batch can supply up to two transcript-linked tips; three open tips pause further checks. Users can dismiss advice or explicitly save follow-ups into the existing task list. Handwritten notes, meeting metadata and manual chat are excluded from automatic prompts. Changing AI destinations or accounts requires a fresh per-call opt-in.

The portable service owns cadence, grounding, deduplication, stale-result checks and explicit task creation. SQLite and in-memory adapters read at most64 effective recent segments, then prompts enforce an encoded byte limit. One native worker and the existing selected provider supply the execution; no separate agent framework or model is added. See [contract](live-coach-contract.md), [native transport limits](live-coach-native.md) and [UI](live-coach-ui.md). Final checks passed3 core unit+83 core integration,120 native library,2 desktop lifecycle,7 UI unit and77 browser behavior tests. Independent final review scored33/40. Native startup, editor draft recovery/cancel, clean quit and relaunch passed; model usefulness and16GB call-time behavior remain unqualified. Details are recorded in [native verification](native-verification.md).

## Release qualification

The minimum comfortable target remains a 16 GB laptop alongside an active video call. Existing synthetic/public-audio measurements on the 32 GiB Mac do not establish this. Real long/noisy calls, Bluetooth/device changes, sleep/wake and permission revocation need checks on each supported desktop OS. Notion sign-in and publishing passed a labeled live-workspace check; ChatGPT and paid hosted processing still need account-level acceptance. Signed/notarized packages, a signed update channel and mobile signing require distribution credentials and supported hardware; development bundles are not a substitute.

We retain one Meeting Workspace, one portable application core and core-owned ports. No new service framework, event bus, always-on embedding model or hosted meeting store was introduced. The Cloudflare Worker handles Notion authorization only.
