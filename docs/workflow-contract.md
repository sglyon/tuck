# Workflow implementation contract

This is the working interface agreement for the next implementation wave. Existing RPCs and DTOs remain valid. Keep one Meeting Workspace, core-owned ports, SQLite transactions, and thin platform/provider adapters. New persisted fields need defaults when opening older libraries. No event bus, service framework, cloud dependency or always-running LLM is introduced.

## Ownership

- Core agent: `crates/tuck-core/**` and its core tests only.
- Workspace UI agent: `src/**`, UI tests, and design artifacts only. Coordinate with root before changing Settings ownership.
- Native workflow agent: `src-tauri/src/**` except `adapters/notion.rs`, `native/**`, native helper/build scripts, Cargo/config/capabilities. Root coordinates shared dependencies.
- Root: integration, Notion adapter, verification, acceptance and backlog. New independent reviewers work after authors finish.

## Meeting additions

Optional/defaulted fields: `folder: string`, `tags: string[]`, `people: string[]`, `tasks: ActionItem[]`, `calendarEvent: CalendarEvent | null`. Existing `summary` becomes explicitly editable through `update_meeting`; personal notes remain independent.

`ActionItem = {id, title, owner, dueDate, done, createdAt}`; `dueDate` is empty or ISO date. `CalendarEvent = {id, source, title, start, end, joinUrl, attendees: string[]}`. Calendar start/end are RFC3339; event IDs include their source namespace.

Settings add `customTemplates: Template[]`, `recipes: Recipe[]`, `microphoneDeviceId: string`, `systemDeviceId: string`. Empty device ID means system default. `Template = {id,name,instructions}`; `Recipe = {id,name,prompt}`. IDs are stable UUIDs. Existing built-in template IDs keep working and receive meaningful per-template instructions.

## Core commands

- `update_meeting {id,changes}` additionally accepts summary/folder/tags/people/calendarEvent.
- `upsert_template {template:{id?,name,instructions}}`, `delete_template {id}` -> custom template array.
- `upsert_recipe {recipe:{id?,name,prompt}}`, `delete_recipe {id}` -> recipe array.
- `add_task {meetingId,title,owner?,dueDate?}`, `update_task {meetingId,id,changes}`, `delete_task {meetingId,id}` -> updated meeting.
- `list_tasks {includeDone?}` -> tasks with `meetingId` and `meetingTitle`.
- `propose_notes {meetingId,instruction,target:"notes"|"summary",requestId}` -> `{text,baseText,target}`; never saves the proposed text automatically.
- `apply_proposal {meetingId,target,baseText,text}` -> updated meeting, only if target still matches baseText. Keep the prior text in lazily loaded revision history.
- `note_history {meetingId}` -> `{id,target,text,createdAt}[]`.
- `search_library {query,folder?,tags?,people?,meetingIds?,limit?}` -> `{meetingId,meetingTitle,segmentId?,start?,text,source}[]`. Bounded evidence lookup; label lexical retrieval honestly.
- `chat_library {prompt,requestId,meetingIds?,folder?,tags?,people?}` -> assistant message with citations linking actual retrieved meetings/segments; `library_chat_delta` event. Keep this conversation separate from each meeting's chat and avoid loading every transcript into memory/context. Current meeting chat also includes the visible recap.

Core agent may extend these with narrowly needed fields after informing UI/native/root. Domain validation and durable writes precede events. Conflict-sensitive AI proposals do not overwrite intervening user edits.

## Desktop commands

The native workflow owner keeps `engine_call` as the UI entry point and routes desktop-only commands through a small runtime wrapper before delegating normal use cases to MeetingService. The diagnostic host should retain the same composition where possible.

- `capture_devices {}` -> `{microphones:[{id,name,isDefault}],systemOutputs:[{id,name,isDefault}]}`.
- `capture_readiness {}` -> `{modelReady,helperReady,microphoneAvailable,systemAvailable,issues:[{code,message}]}`. Never starts recording or requests permissions merely on launch.
- `test_audio {microphone,systemAudio}` -> a bounded five-second explicit device test with levels/events and readable results; never persists or transcribes test audio.
- `calendar_status {}` -> `{available,connected,source,message?}`.
- `connect_calendar {}` -> status after explicit OS calendar permission on macOS.
- `calendar_events {from,to}` -> CalendarEvent array, bounded date range.
- `import_calendar {path}` -> CalendarEvent array from a chosen local ICS file for portable fallback. Be clear that this is import, not Google/Outlook OAuth sync.
- `hide_window {}` and `quit_app {}` for a persistent tray app after frontend note flushing.
- Global dictation status/toggle/cancel and explicit Accessibility settings use the [dictation host contract](dictation-contract.md). The shortcut saves a microphone-only voice note, copies original speech and conditionally sends a paste to the same macOS field; Windows/Linux use manual paste. No additional model, AI/network processing or automatic publication is introduced.

Native Calendar uses EventKit on macOS, local ICS import elsewhere. Direct Google/Outlook OAuth needs a registered client and must not be simulated. Root/native agent agree exact events for global shortcut/tray activation and guarded quit before implementation. Recording starts only through an explicit user action, never solely because a calendar event exists.

RecordingPort may gain a default `configure(settings)` method so existing adapters/tests remain compatible; the application calls it before starting capture. Native agent owns actual device selection, test and permission handling. All CPAL handles remain owned by their capture thread.

## UI requirements

Preserve the existing visual world and straightforward meeting desk. Add useful navigation and progressive disclosure, not a dashboard of technical controls. Saved templates, recipe execution, editable recaps, reviewable note proposals, organization, cross-meeting answers and task follow-through must be reachable in ordinary language. Calendar/readiness/quick controls plug into the agreed desktop commands. Use genuine empty/error/busy states and protect ordered saves/close flushing. Keep browser preview clearly isolated.

Every changed UI area receives an Impeccable confirmation review and must meet the user's 30/40 threshold before handoff. Provider consent and examples retain their existing protections.

## Remaining backlog after this wave

Richer Notion updates and setup; audio playback/corrections and in-app recording recovery; custom vocabulary and broad import; backup/restore; mobile/device continuity; sharing/collaboration; Windows/Linux dictation insertion, shortcut customization and snippets; release signing/updating; real meeting and 16 GB qualification. Root keeps these visible rather than declaring them complete when only a first slice exists.
