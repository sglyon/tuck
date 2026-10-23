# Meeting workflow progress

This extends the competitive research in [research.md](research.md). Implemented behavior, mock/integration verification and real platform qualification are different kinds of evidence. See [workflow-core.md](workflow-core.md), [workflow-ui-review.md](workflow-ui-review.md) and [native-verification.md](native-verification.md) for the relevant checks.

## Implemented in the workflow wave

| Need | Implementation | Limit |
| --- | --- | --- |
| Quick capture | Persistent tray, explicit record shortcut, setup gating, save before hide/quit | No automatic detection of meeting apps or ambient recording |
| Meeting context | Read-only macOS EventKit connection, upcoming events, prepare title/attendees, portable ICS import | Device calendar needs explicit permission; ICS is a snapshot without recurrence expansion; no direct Google/Outlook OAuth |
| Capture confidence | Device selection, preflight readiness, explicit five-second sound check, silence warnings | Real microphones/system capture and permission behavior still need hardware qualification |
| Human-owned notes | Separate editable recap/notes, reviewable AI proposals, conflict guards, periodic manual checkpoints and exact pre-replacement versions | AI output still needs review; transcript corrections preserve the original speech separately |
| Reusable work | Meaningful built-in/custom recap templates and saved recipes | Recipes produce reviewable text; they do not send external messages |
| Follow-through | Per-meeting actions with owner/date/completion and a task inbox | No external task-system synchronization or automatic commitment creation |
| Organization | Folders, tags and people scopes | People labels do not establish speaker identity |
| Library recall | Bounded lexical evidence retrieval, cross-meeting questions, actual source links | No semantic index, persistent library conversation history or unbounded transcript context |
| Notion | Shared-destination discovery, schema-aware optional mappings, known-page updates, read-only uncertain-page recovery | Internal integration token; registered public OAuth is not configured; unknown create outcomes never trigger a blind duplicate upload |

Notion updates preserve content outside marker-verified Tuck sections. A complete replacement is read back before an earlier section is retired. Reconciliation compares actual content rather than trusting a revision label. Older flat exports stay in place; the first update adds a managed section. Mock HTTP tests cover ambiguity, actual-content reuse, partial replacement, foreign sections, bounded cursors and read-only recovery. These tests do not claim a live Notion account was exercised.

## Next product work

The subsequent library-safety increment implements retained-audio playback, guarded transcript corrections/reset with original speech preserved, linked recovery workspaces and portable ZIP backup/preview/merge restore. Its UI review meets the floor at 32/40, 31/40 and 32/40. An isolated native integration check made 131 requests with real Parakeet/public speech and verified exact audio bytes after restore, stale correction/fingerprint rejection and preservation of current edits on repeat restore. See `benchmarks/native-workflow/library-safety.json`, [safety contract](library-safety-contract.md) and [UI review](safety-ui-review.md).

Broader file import is also implemented: PCM WAV, MP3, FLAC and AAC-LC stream through bounded local conversion without modifying the selected source. Codec fixtures and a public AAC-to-Parakeet integration passed. AAC edit-list trimming remains unsupported, so priming/padding is retained. See [audio import](native-audio-import.md).

1. Custom vocabulary with a validated recognition path; [evaluation](vocabulary-evaluation.md) records actual backend requirements rather than presenting a no-op setting.
2. Meeting-app detection/reminders, persistent voice identity and better overlap/echo handling, each with realistic privacy and accuracy checks.
3. Native mobile capture and device continuity; hosted synchronization/sharing require account ownership, offline conflicts and retention decisions.
4. Optional cross-application dictation and snippets, once the primary meeting workflow is reliable.

## Release qualification

The minimum comfortable target remains a 16 GB laptop alongside an active video call. Existing synthetic/public-audio measurements on the 32 GiB Mac do not establish this. Real long/noisy calls, Bluetooth/device changes, sleep/wake and permission revocation need checks on each supported desktop OS. Actual provider sign-in/publishing also needs account-level validation. Signed/notarized packages, a signed update channel and mobile signing require distribution credentials and supported hardware; development bundles are not a substitute.

We retain one Meeting Workspace, one portable application core and core-owned ports. No new service framework, event bus, always-on embedding model or hosted backend was introduced to deliver this wave.
