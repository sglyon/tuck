# Library loading contract

The desktop uses `bootstrap_library`, never full-library `bootstrap`. Full bootstrap remains for diagnostic compatibility. A selected conversation still loads one complete meeting; this change bounds cross-library transfer and avoids decoding evidence before selection.

## Commands and DTOs

- `bootstrap_library {limit?:50}` → `{meetingPage,library,settings,status,accounts}`.
- `list_meetings {limit?:50,cursor?:string|null,query?:string,starred?:boolean,scope?:Scope}` → `MeetingPage`.
- `get_meeting {meetingId}` → the existing complete Meeting DTO, including corrected transcript, speakers, messages, bookmarks and tasks. IDs need not occur on the current page.
- `library_overview {}` → `LibraryOverview`.
- `list_task_page {limit?:50,cursor?:string|null,includeDone?:false,scope?:Scope}` → `{tasks,nextCursor,total}`; tasks retain existing ActionItem fields plus `meetingId,meetingTitle`.

`Scope` is `{folder?:string,tags?:string[],people?:string[],meetingIds?:string[]}`. Folder is exact; all requested tags and people must match; meeting IDs form an optional union. Empty folder means no folder constraint. Arrays are normalized by sorting/deduplicating. Text query is a trimmed contiguous substring in title, notes, recap or effective corrected transcript; case folding is ASCII, with other Unicode compared exactly. Query results cover the entire library, not only loaded pages.

`MeetingPage` is `{meetings:MeetingHeader[],nextCursor:string|null,total:number}`. Headers contain `id,title,createdAt,duration,status,template,starred,isExample,notionUrl,notionStatus,folder,tags,people,calendarEvent,recoveredFrom` and `segmentCount,speakerCount,messageCount,bookmarkCount,taskCount,openTaskCount,hasNotes,hasSummary`. They contain no note/recap bodies, tasks, transcript, speaker details, messages or bookmarks. Older metadata defaults match the full Meeting DTO.

`LibraryOverview` is `{meetingCount,starredCount,openTaskCount,folders:string[],tags:string[],people:string[],facetsTruncated:{folders:boolean,tags:boolean,people:boolean},activeMeetingId:string|null}`. Counts are exact across the whole library. Each facet array is globally distinct, sorted and bounded to 200 entries; its flag explicitly reports omitted choices. Active means recording, paused or processing, with deterministic newest-first selection if old inconsistent records contain multiple active meetings. Startup recovery interrupts abandoned work before this response. Global task APIs remain independent of header pages.

Limits are strict integers from 1 through 100, default 50. Normalized filters, including JSON escaping, have a 32,000-byte serialized budget; oversized scopes are rejected with a request to select fewer values. This guarantees every generated cursor can be replayed within its 128,000-character input cap. Meeting order is `createdAt DESC,id DESC`, including timestamp ties. Tasks retain priority order: incomplete first, dated before undated, due date ascending, creation ascending, then meeting ID and task ID ascending. Pagination uses deterministic keyset positions; no offset scan or per-meeting full DTO fetch occurs. `total` is the full filtered count before cursor position.

## Cursor and mutation behavior

Cursors are opaque versioned strings, bound to normalized filters, collection kind and a durable library generation. Reusing a cursor with different filters is invalid. A relevant mutation returns `cursor_stale`; clients discard that page chain and reload the first page. Limit can change without changing selection. New/deleted meetings, metadata or note/recap changes, incoming segments, transcript corrections, and speaker/message/bookmark changes invalidate cursors. Revision history/settings alone do not. Generation changes commit with the transaction; rollback leaves cursors valid. Cursor strings are untrusted, validated and bounded, and provide no authorization or persistent snapshot guarantee.

Existing `meeting_updated` events keep their full DTO for compatibility. The desktop may merge the header, retain full details only for selected/active/pending-edit meetings, invalidate other detail cache entries, and debounce a first-page/overview refresh. Segment events remain unchanged. A complete off-page meeting can always be requested directly. Facets/counts must come from `library_overview`, and the task inbox from `list_task_page`, never inferred from current pages.

## Storage and startup

SQLite projects header fields in SQL and selects bounded pages directly, using indexed timeline order and counts without loading evidence documents. Nonempty text searches inspect only local evidence as needed for matching and never return all bodies. An additive generation table/triggers and timeline index preserve existing schema-3 document bytes and backup compatibility. Recovery queries select only abandoned active/publication-syncing records instead of loading all note/recap documents during service construction. No remote service, search embeddings, framework or second write model is introduced.
