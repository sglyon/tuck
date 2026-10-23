# Meeting Workspace workflow implementation

This increment extends the existing Meeting Workspace context. The architecture and pattern-selection record in `architecture.md` still apply: meeting understanding is core, capture and publishing are supporting, and persistence/inference remain generic adapters. Tasks, organization and saved prompts do not introduce another bounded context, event bus, repository framework or hosted dependency.

## Durable edits and proposals

Meeting metadata now includes defaulted folder, tags, people, tasks and an optional calendar event. Older JSON documents deserialize without a rewrite. Summary is directly editable through `update_meeting`, separately from personal notes. Metadata validation is atomic: an invalid task date or calendar event cannot partially apply another edit.

Tasks belong to their meeting, have stable UUIDs, and support explicit create/update/delete and a global open/all task list with meeting references. Owners and ISO due dates are user-supplied. AI output never silently creates commitments or marks tasks done. Tags and people are bounded, deduplicated lists. Calendar events require source-qualified IDs, RFC3339 bounds and HTTP(S) join links.

`propose_notes` reads a snapshot and returns proposed text, target and exact base text. It saves nothing automatically. `apply_proposal` checks the current target against that base in the same transaction as the write. An intervening manual edit returns `conflict`. Manual note/recap edits retain periodic prior-text checkpoints in `note_revisions`: the first manual edit, the first edit after a semantic replacement, or an edit at least five minutes after the latest manual checkpoint. Accepted proposals and generated recaps always retain the exact prior text. Sub-five-minute typing states are intentionally coalesced; draft saving itself remains immediate/ordered as before. `summarize` also guards the recap snapshot against an intervening edit. Revision insertion and target changes commit or roll back together before `meeting_updated` is emitted.

History is loaded only through `note_history`, never added to meeting/bootstrap DTOs. Storage retains revisions; this first history read returns the newest 100. An autosave consults only latest revision metadata (kind and timestamp), never those full history bodies. An additive SQLite expression index supports the meeting/target metadata lookup. Future or invalid checkpoint timestamps create a fresh checkpoint. Legacy records without a kind are treated as semantic replacements. Meeting deletion cascades revisions. Future pagination can expose older entries without inflating the ordinary workspace load.

## Templates and recipes

Built-in general, sales, standup, interview, one-on-one and planning instructions differ by purpose. Custom templates have stable UUIDs, names and explicit summary instructions. Summary generation reads those instructions from settings.

Three initial recipes cover decisions/actions, a reviewable follow-up draft, and risks/open questions. Their UUIDs are fixed across restarts. Users can edit/delete them and save additional recipes. The UI runs a recipe's prompt through the existing meeting chat or proposal workflow. Recipes never send a message externally. Template/recipe writes validate all saved items before replacing settings, preserving other settings and native credential separation.

## Bounded lexical library retrieval

`search_library` uses lexical retrieval, not embeddings or semantic search. Up to 12 literal word tokens match title, notes, recap or transcript text. Results are ranked by distinct matching query terms. Matching uses SQLite-compatible ASCII case folding; non-ASCII text is matched literally. Tokens are bound parameters and cannot execute SQL or supply LIKE/FTS syntax.

Folder equality, all selected tags, all selected people and optional meeting IDs intersect to form the scope. SQLite scans the scoped evidence within the database and returns at most 30 ranked rows (12 by default); it does not load all library transcripts into application memory. Each returned evidence excerpt is at most 1,200 characters and includes its actual meeting, source and optional segment/timestamp. The memory adapter retains the same bounded best-result set during scanning. SQLite lexical scans are intentionally a proportionate first implementation; a future FTS5 index can improve latency on larger libraries without changing the core port.

`chat_library` retrieves at most 12 excerpts and puts only those excerpts in its evidence context. No matches produces a clear response without invoking a model. Web search is disabled for this use case. Library answers return structured `citations`, `retrieval: "lexical"`, and canonical source links generated from repository results. Model-supplied Markdown link destinations are removed before known evidence markers are linked. Sources are labeled as retrieved evidence rather than an assurance that every generated claim is proven. No library answer is added to an individual meeting's message history. The current contract handles a question at a time; the UI owns its separate library conversation display.

Citation URLs are:

- Transcript: `tuck://meeting/{meetingId}?segment={segmentId}&start={seconds}&source=transcript`
- Other evidence: `tuck://meeting/{meetingId}?source=title|notes|summary`

IDs/query values are URL encoded. Existing per-meeting chat now includes the visible recap, clearly labeled as interpretation, alongside personal notes and bounded transcript evidence.

## Native and publication seams

`RecordingPort.configure(settings)` has a compatibility default and runs before changing the meeting into a recording state. Device IDs default to empty strings (system defaults). A configuration failure leaves the meeting in draft and does not activate capture.

`PublicationPort` now has compatibility defaults for destination discovery/inspection and `publish_with_settings`. Native adapters receive the validated `notionPropertyMapping` without shared mutable configuration. `publish_notion` keeps its initial-create duplicate and uncertain-outcome protections. Explicit `sync_notion` requires a known page URL and allows safe reconciliation after an uncertain update. An uncertain initial create with no URL remains blocked. Discovery and inspection delegate to the publication adapter through `notion_destinations` and `notion_destination`.

## Migration and validation

SQLite schema version 2 adds only the revision table/index; existing meeting/segment/settings layout stays intact. Schema creation/migration runs atomically. Opening a v1 library supplies new JSON defaults, preserves the original document until an explicit save, and preserves optimistic version checks. Newer unsupported schema versions still fail clearly. A database backup must retain the whole SQLite database (and use SQLite backup or a stopped/checkpointed WAL library), as before; this increment does not claim a complete user-facing backup/restore flow.

Validation: 31 core tests pass across the memory and SQLite adapters, including repository version/rollback/evidence contracts, old-library migration, metadata/tasks, proposal conflicts, prior-text rollback/cascade, recap concurrency, templates/recipes, bounded/scoped retrieval and real citations, capture configuration ordering, and known-page versus uncertain-create publication. Strict Clippy with all targets passes. Provider/native integration and browser UI verification remain with their respective owners.
