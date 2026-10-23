# In-app voice notes

Voice notes reuse the Meeting Workspace, recording lifecycle, installed local Parakeet engine, and existing transcript evidence. They add no model, service, separate table, or context. Private capture remains a supporting capability; storage and inference remain generic. The existing small Meeting aggregate protects lifecycle, immutable kind, and editable notes. The existing core-owned RecordingPort keeps this workflow testable without a microphone or inference runtime. This applies `ddd-selecting-patterns`, `ddd-aggregate`, and `ddd-hexagonal-architecture` proportionately within the architecture already described in `architecture.md`.

## RPC and DTO contract

- `create_dictation({title?: string})` creates a draft workspace; omitted title defaults to `Voice note`. It returns the normal complete Meeting DTO with `kind: "dictation"`.
- `create_meeting` continues to create `kind: "meeting"`. Missing kind in older SQLite documents and portable backups defaults to `meeting`. Detail and paginated library header DTOs both include kind. The existing `update_meeting` changes do not permit editing kind.
- `capture_readiness({kind: "dictation"})` uses the installed speech engine and selected microphone. It retains `modelReady`, `helperReady`, `microphoneAvailable`, `systemAvailable`, `permissions`, and `issues`. System capture availability, system permission, the optional speaker model, and the ScreenCaptureKit audio helper do not gate voice notes. macOS still needs the permission-status helper and, for the installed Core ML backend, its speech helper. Readiness never starts capture or requests permission.
- Existing `start_recording`, `pause_recording`, `resume_recording`, `stop_recording`, `update_meeting`, detail loading, deletion, audio recovery, and backup/restore RPCs are reused. A dictation start always selects microphone on and system audio off, regardless of caller input. Its transient `RecordingPort.configure` settings set `diarization: false`; this does not change saved preferences. Ordinary meetings keep the existing native default of speaker recognition when installed.
- Live recognized speech arrives through the existing `segment` events and `segments` DTO. Stop drains final speech before completing. If notes are still empty, completion seeds editable `notes` with raw speech text in timeline order, separated by spaces. The same rule preserves available text on partial start failure, asynchronous failure, shutdown, startup recovery, and restored interrupted data. The editable note is bounded to the existing 200,000-character note limit; original segment evidence stays complete and unchanged.
- Existing nonempty notes are preserved through stop, interruption, and audio recovery. Recovery creates a new linked workspace with the original kind and notes, leaving its source untouched. Rejected lifecycle retries cannot replace notes.
- The UI copies the current editable notes as plain text. A user's note edits never rewrite the original transcript segments.

## Boundaries and lifecycle

One capture lifecycle guard provides mutual exclusion across meetings, voice notes, imports, recovery, backup/restore, and shutdown. A pristine failed start stays draft; partial evidence stays interrupted and recoverable. The voice-note path uses the same native capture and inference preflight as meetings, with microphone-only requirements and no speaker extractor initialization.

Stopping or recovering a voice note never automatically invokes recap generation, a language model, or publication, even when global `autoSummary` or `notionAutoSync` settings are enabled. Voice notes stay local unless the user invokes another explicit workflow. Capture occurs only after the user's in-app start action. System-wide paste and global dictation are outside this feature.

## Verification

Core tests use fake recording, intelligence, publication, and archive adapters against both in-memory and SQLite stores. They verify microphone-only settings, lifecycle exclusion, editable notes with preserved raw speech, no automatic AI/publication, typed-note preservation, pristine and partial start failure, shutdown/startup/audio recovery, and backup compatibility. A native speaker-loader test uses an installed-size corrupt fixture to prove disabling diarization bypasses optional speaker-model verification/loading. These tests use no personal microphone or system audio.
