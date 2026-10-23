# Library safety implementation contract

One Meeting Workspace context; core owns evidence/correction/restore rules, native owns filesystem and ZIP/audio safety, UI owns explicit editing and file pickers. Existing RPCs remain supported. This is the working agreement for the next wave.

## Corrections

`correct_transcript {meetingId,segmentId,baseText,text}` and `reset_transcript {meetingId,segmentId,baseText}` return the updated meeting. `baseText` is the exact currently displayed segment text; stale edits return `conflict`. Reject correction/reset while the meeting is active. Text must contain 1–32,000 characters. Reset removes the separate correction record.

Raw ASR segment records remain immutable. Corrections are persisted separately as `{segmentId,text,updatedAt}`. Meeting segment DTOs expose effective `text` and `edited: boolean`; only corrected passages include immutable `originalText` and `correctedAt`. Unedited passages omit those duplicate fields; callers use `originalText ?? text` to read original recognition. Saving unchanged original text does not create a correction. Existing callers continue to use `text`. Search, AI context and exports use the effective evidence with an explicit user-corrected marker; JSON export also retains originalText for corrected passages. Reset-to-original is the first correction recovery path; a separate correction revision timeline is deferred. Note/recap history retains periodic manual checkpoints plus every accepted AI replacement. Draft durability and base-text conflict checks are unchanged. Optional revision `kind` is `manual` or `replacement`; older records without a kind remain readable and are treated as replacements for checkpoint decisions.

Meeting metadata adds defaulted `recoveredFrom: string | null` for new recovery workspaces.

## Retained audio and recovery

`meeting_audio {meetingId}` returns `[{id,name,bytes,duration?,repairable?}]`, where `id` is the exact owned filename (`microphone.wav`, `system.wav` or `import.wav`). Empty results truthfully mean no retained recording is available. `audio_source {meetingId,audioId}` validates through core/native ownership and returns native `{url}` using a scoped `tuck-audio://localhost/{meetingUUID}/{audioId}` playback protocol. The native runtime replaces the core's internal resolved `{path}` result with that URL; no arbitrary path is accepted from the webview.

`recover_audio {meetingId,audioId}` requires an inactive, non-example source and no concurrent capture. It imports the selected retained WAV through the existing transcription pipeline into a **new** meeting titled `Recovered · {title}`, with `recoveredFrom` linked to the original. Personal notes, organization and calendar context copy to the new workspace; recap/publication state starts clean. The original notes, speech evidence, corrections and audio remain untouched. Recovery never appends another transcript to the original. Automatic publishing is suppressed for this recovery operation.

Core `AudioArchivePort` additions have compatibility defaults:

- `list(meeting_id: &str) -> Result<Vec<Value>>`
- `resolve(meeting_id: &str, audio_id: &str) -> Result<String>` (validated physical path, internal only)
- `write_backup(path: &str, manifest: &Value, include_audio: bool) -> Result<Value>`
- `read_backup(path: &str) -> Result<Value>` → `{manifest,stagingToken,audioCount,fingerprint,warnings?}`
- `commit_restore_audio(staging_token: &str, id_map: &Value) -> Result<Value>` → `{audioFilesRestored,warnings: string[]}`
- `discard_restore(staging_token: &str) -> Result<()>` (default no-op)

Native validates a UUID-owned regular WAV under the app audio root, rejects symlinks and arbitrary filenames, and uses bounded Range responses for playback. Recoverable WAV-header repair, if needed, operates on a temporary recovery copy, never on the original. Real audio/device behavior requires separate native validation; browser fixtures establish only the interface contract.

## Portable backup manifest

`backup_library {path,includeAudio?}` defaults includeAudio to true and returns `{path,meetingCount,audioIncluded,audioFiles?,warnings:string[]}`. Core takes one consistent SQLite snapshot; native writes a new portable ZIP with `manifest.json` and optional validated meeting-owned audio. No credentials, account tokens or native secret-store data enter the archive.

Manifest shape:

```
{format:"tuck-library",version:1,createdAt,settings,
 meetings:[{meeting,segments,corrections,speakers,messages,bookmarks,revisions}]}
```

`meeting` is metadata (including tasks/calendar/recovery linkage). `segments` contain original ASR text; `corrections` are separate records. `revisions` includes the full retained note/recap history, not only the newest 100 read by the UI. Native may add an `audio` index; it never changes evidence text. `settings` contains only known nonsecret settings; credential presence flags are excluded. Hosted consent and automatic actions are exported disabled, provider defaults local, and device IDs/Notion destination/mapping are cleared for portability.

## Merge restore

`preview_restore_library {path}` validates and stages a chosen archive, returns `{meetingCount,restorable,skipped,audioFiles,warnings,settingsMode,fingerprint}`, then discards staging without writing the database. Fingerprint is SHA256 of the raw manifest bytes. Actual restore requires `{path,expectedFingerprint}` and rejects a changed manifest before writing. Audio is independently revalidated.

`restore_library {path,expectedFingerprint}` returns `{restored,skipped,audioFilesRestored,warnings:string[],settingsMode}`. Native validates and stages the ZIP first; core validates the entire manifest before a database write. Unsupported formats/versions or malformed evidence fail without a partial metadata import.

Restore preserves backed-up meeting/segment/task UUIDs and imports **only meeting IDs absent from the current library**. Existing meeting IDs are skipped as a unit; no current meeting or evidence is overwritten or deleted. Repeating the same restore is idempotent. The native audio ID map is an object mapping each imported old meeting ID to its identical new ID; skipped IDs are absent. This preserves stable identity rather than duplicating the library on every restore.

Restored active statuses become interrupted, and publication status/URL is reset to idle/empty. No restored operation records, credentials, hosted consent or automatic actions become live. For an empty target library, validated safe preferences are restored with provider local and all hosted/automatic actions disabled. For an existing library, current settings remain in force; only previously absent custom template/recipe IDs merge. Saved-writing collisions preserve the current item. Device IDs and Notion target/mapping are never restored.

Core commits all new metadata/evidence in one transaction, then asks native to copy staged audio for imported IDs only. A post-commit audio failure returns a successful metadata restore with explicit warnings and actual audio-file counts. It does not claim rollback of committed metadata or filesystem/database atomicity. Staging cleanup runs after success or failure. No audio overwrites an existing destination directory; conflicting/orphan audio is reported rather than replaced.

## Ownership and verification

- Core: `crates/tuck-core/**`, this contract and core design/test notes.
- Native: archive/playback protocol and ZIP staging in `src-tauri/src/**`, dependencies/capabilities. Core never imports a host filesystem or ZIP framework.
- UI: correction/reset editor, original-evidence disclosure, retained audio controls/recovery action, backup save/restore open flow and truthful reports.
- Root: integration, independent checks and final acceptance.

Required tests include memory+SQLite correction conflicts/raw preservation/reset/cascade, correction-aware retrieval/export, v1/v2 migration without document rewrite, complete revision backup, idempotent/skipping restore, invalid-manifest rollback, safe restored preferences, untouched current meetings and explicit partial-audio reports. No complete-library transcript is sent to an LLM.
