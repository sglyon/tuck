# Voice notes and global dictation

Voice notes reuse the Meeting Workspace, recording lifecycle, installed local Parakeet engine, and existing transcript evidence. They add no second speech model, SQLite table or domain context. Private capture remains a supporting capability; storage and inference remain generic. The existing small Meeting aggregate protects lifecycle, immutable kind, and editable notes. The existing core-owned RecordingPort keeps this workflow testable without a microphone or inference runtime. This applies `ddd-selecting-patterns`, `ddd-aggregate`, and `ddd-hexagonal-architecture` proportionately within the architecture already described in `architecture.md`.

## RPC and DTO contract

- `create_dictation({title?: string})` creates a draft workspace; omitted title defaults to `Voice note`. It returns the normal complete Meeting DTO with `kind: "dictation"`.
- `create_meeting` continues to create `kind: "meeting"`. Missing kind in older SQLite documents and portable backups defaults to `meeting`. Detail and paginated library header DTOs both include kind. The existing `update_meeting` changes do not permit editing kind.
- `capture_readiness({kind: "dictation"})` uses the installed speech engine and selected microphone. It retains `modelReady`, `helperReady`, `microphoneAvailable`, `systemAvailable`, `permissions`, and `issues`. System capture availability, system permission, the optional speaker model, and the ScreenCaptureKit audio helper do not gate voice notes. macOS still needs the permission-status helper and, for the installed Core ML backend, its speech helper. Readiness never starts capture or requests permission.
- Existing `start_recording`, `pause_recording`, `resume_recording`, `stop_recording`, `update_meeting`, detail loading, deletion, audio recovery, and backup/restore RPCs are reused. A dictation start always selects microphone on and system audio off, regardless of caller input. Its transient `RecordingPort.configure` settings set `diarization: false`; this does not change saved preferences. Ordinary meetings keep the existing native default of speaker recognition when installed.
- Live recognized speech arrives through the existing `segment` events and `segments` DTO. Stop drains final speech before completing. If notes are still empty, completion seeds editable `notes` with raw speech text in timeline order, separated by spaces. The same rule preserves available text on partial start failure, asynchronous failure, shutdown, startup recovery, and restored interrupted data. The editable note is bounded to the existing 200,000-character note limit; original segment evidence stays complete and unchanged.
- Existing nonempty notes are preserved through stop, interruption, and audio recovery. Recovery creates a new linked workspace with the original kind and notes, leaving its source untouched. Rejected lifecycle retries cannot replace notes.
- In-app Copy text copies the current editable notes as plain text. Original speech → Copy original speech copies the full original recognized segments independently of note edits or the bounded editable note. A user’s edits never rewrite original speech.

## Boundaries and lifecycle

One capture lifecycle guard provides mutual exclusion across meetings, voice notes, imports, recovery, backup/restore, and shutdown. A pristine failed start stays draft; partial evidence stays interrupted and recoverable. The voice-note path uses the same native capture and inference preflight as meetings, with microphone-only requirements and no speaker extractor initialization.

Stopping or recovering a voice note never automatically invokes recap generation, a language model, or publication, even when global `autoSummary` or `notionAutoSync` settings are enabled. Voice notes stay local unless the user invokes another explicit workflow. Capture starts only through a deliberate in-app, tray or registered shortcut action. There is no ambient monitoring or automatic launch-time recording.

## Global dictation host contract

The native host registers `CommandOrControl+Shift+Space`: press to start, press again to finish. Shortcut handling keeps the original application in focus and reuses the same microphone-only core voice-note lifecycle. The controller owns one transient session; it introduces no additional speech model, network/AI call or Notion publication. Capture remains mutually exclusive with meetings and other voice notes. Startup is idle, and shutdown saves/drains owned capture without delivery.

- `global_dictation_status {}` returns `{shortcut,shortcutAvailable,state,message?,meetingId?,pasteAvailable,pastePermission}`. State is `idle|starting|listening|processing|copied|paste_sent|error`; permission is `granted|denied|unavailable|unknown`. This read never requests Accessibility permission or begins capture.
- `global_dictation_toggle {}` starts or finishes the owned session. `global_dictation_updated` carries the status DTO directly. Native shortcut dispatch works independently of the webview.
- `global_dictation_cancel {}` cancels delivery, drains capture and saves the note without copying or sending a paste. It does not stop another meeting’s recording. Stopping the owned voice note through its ordinary document controls also retires global delivery.
- `dictation_accessibility_settings {}` opens the relevant OS settings only after the user’s explicit action. Accessibility is required for automatic macOS paste, not transcription or copying.

Successful completion assembles original speech in timeline order after final capture drain, rather than using editable notes or transcript corrections. Empty, interrupted, invalid or oversized results do not change the clipboard; their retained note/evidence remains reviewable. Automatic delivery is bounded to 200,000 characters and never silently truncates; full original speech remains available in Tuck.

macOS retains only the original app/field identity. It confirms an eligible editable field at start and again before dispatch, rejects protected or changed targets, and waits briefly for shortcut modifiers to release. It never reads field text or the previous clipboard value. Successful text is copied first; an unavailable, changed or uncertain target leaves manual paste available. `paste_sent` means a paste event was dispatched, not verified insertion. Windows/Linux currently copy without automatic insertion. Shortcut conflicts and desktop-session restrictions are reported instead of implying registration succeeded.

The in-app global controls keep insertion deliberate: Start dictation here produces a saved/copied note; starting from another app with the shortcut provides the potential target. Neither route replaces Apple’s speech engine, installs a system dictation service, or remaps Fn/Globe.

## Verification

Core tests use fake recording, intelligence, publication, and archive adapters against both in-memory and SQLite stores. They verify microphone-only settings, lifecycle exclusion, editable notes with preserved raw speech, no automatic AI/publication, typed-note preservation, pristine and partial start failure, shutdown/startup/audio recovery, and backup compatibility. A native speaker-loader test uses an installed-size corrupt fixture to prove disabling diarization bypasses optional speaker-model verification/loading. These tests use no personal microphone or system audio.

Global-controller and delivery tests use synthetic recording/field/clipboard adapters; browser fixtures verify status ownership, explicit controls, fallback copy and one stable polite announcer. These checks do not establish real microphone capture, OS permission behavior, focus retention or target-app insertion. See [UI evidence](global-dictation-ui.md) and the separate native acceptance record before making platform qualification claims.

## User configuration

Settings → Audio & models → Dictation anywhere exposes enable/disable, a recorded shortcut (F1–F24 work without modifiers), and automatic paste versus clipboard-only. Changes apply when saved and persist locally in SQLite under `globalDictation`. Existing libraries default to enabled, CommandOrControl+Shift+Space, and automatic paste. In-app voice notes are independent of the global enable switch.

`global_dictation_configure {enabled,shortcut,autoPaste}` validates the key combination, preserves the meeting-recording shortcut, and rejects changes during a prepared or active dictation action. Native registration reserves the replacement before releasing the old shortcut; conflicts and failed persistence roll back. Disabling releases the shortcut. Clipboard-only mode does not capture a focused field and never sends a paste. The status/event DTO adds `enabled` and `autoPaste`; `shortcut` reflects the saved preference. No model download, recording or permission prompt is triggered by saving preferences.

Global dictation shows a read-only, click-through floating indicator at the bottom of the current screen. It never takes keyboard focus. Preparing, Listening and Transcribing stay visible; Copied/Paste sent disappears after three seconds, errors after seven. No dictated text is shown.

When global dictation is enabled, the desktop host prepares the verified local speech engine in the background and reuses it between dictations. This keeps model loading off the usual shortcut path; the microphone remains closed until explicitly started. Unload, model installation, disabling global dictation, and shutdown discard the idle engine. An immediate first use during warmup can still wait for initialization.
