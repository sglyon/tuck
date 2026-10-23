# Guided Notion meeting database

The connected Notion settings surface now offers **Create my meeting database** as its primary first-use action. A user chooses a page already shared with Tuck, reviews the useful meeting columns, and explicitly creates **Tuck meetings** inside that page. Finding shared pages happens when setup opens, rather than on app launch. A secondary **Use an existing database** disclosure retains the prior destination search.

The template preview names meeting name, meeting date, folder, tags and people, with recap, personal notes and action items inside each published meeting. It explains the Tuck meeting ID used for updates. Setup does not invent meeting records or upload existing notes. The automatic publishing preference remains unchanged.

A successful setup refreshes authoritative settings after native code has atomically saved the target and property mappings. The persistent ready state shows the database name, any saved layout warning, and an **Open database in Notion** link. Existing selected databases also show their loaded name. Compatible column mapping remains in its existing optional disclosure, with copy explaining that Tuck's template maps it automatically. A schema is bound to the exact requested destination; changing targets clears the previous schema, and a failed read never exposes stale mapping controls.

An unresolved native setup journal survives leaving Settings and restarting the app. The UI offers **Check for my database**, backed by the same native RPC in reconciliation mode. It never offers a fresh creation while an attempt remains unresolved. An alternate **Find existing database** path searches for Tuck meetings. Definitive failures refresh the journal as well, so the native boundary decides whether creation can safely be attempted again. A saved journal's earlier parent never overrides a new user-selected page.

## Boundary contracts

- `notion_parent_pages {query?}` returns `{pages:[{id,title,url}],hasMore}`.
- `notion_create_database {parent}` returns `{destination,settings}`. The component calls `onUpdate({})` to refresh; it does not save a returned stale settings snapshot.
- Read-only `settings.notionSetup` provides parent, creating/uncertain/ready status, optional destination and message. Repeating an unresolved setup is a native read-only reconciliation operation.
- Existing `notion_destinations`, `notion_destination`, and `update_settings` contracts remain available for existing destinations and compatible metadata mapping.

## Verification

Ten focused browser tests passed: explicit parent/create and complete mapping, unchanged default automatic publishing, preservation of an already enabled preference, prior ready parent versus new selected parent, no authorized pages, page-search network failure, uncertain creation and reconciliation without another remote create, resumed setup with existing-database recovery, the separate existing-database route, persisted layout warnings after reopening Settings, and changing from a readable to an unreadable database without stale columns. Nine incumbent connection/dictation behaviors also passed in the preceding targeted run. TypeScript checking passes.

The tests use strict illustrative native ports and local browser fixtures. They do not establish Notion service behavior, actual credential handling, or native WebKit behavior.

Impeccable's established Operate world was preserved: flat settings, system typography, cornflower action, ordinary form controls, and progressive disclosure. The skill context loader had already run in this session; new-work, onboarding, Operate and craft-floor references were read. The component static detector returned an empty finding list; this is scoped to the TSX component and does not claim an app-wide clean detector result.

The bounded screenshot batch covers entry, selected parent/preview, ready/mapping and unresolved setup at 1320 and 390 pixels: `.impeccable/review/notion-template-{entry,parent,ready,uncertain}-{1320,390}.png`. A compact fixture navigation issue was fixed to finish the missing compact captures. The single settled-state confirmation replaced only the ready frames after dismissing the transient success toast, exposing the lower mapped controls. Independent review then identified two material P2 defects: the persisted ready warning was hidden, and a prior schema could survive a target change. Both were fixed in one batch with regression tests. The orchestrator requested a narrowly scoped functional confirmation for these corrections: `notion-template-{warning,schema-error}-{1320,390}.png`. No additional cosmetic iteration followed. All captures identify their illustrative fixture. Independent scoring is recorded separately by the orchestrator's reviewers.

The independent B review sealed at **32/40**. Its remaining nonblocking limits are deferred: a prior unresolved setup panel can dominate the screen after the user chooses a working existing database; a resumed attempt does not retain the original parent page title; and some explanatory copy repeats. These do not reopen the bounded interface cycle. No P0/P1 interface findings remain in that report. The two independent A material P2 corrections above passed all ten behavioral tests and the narrow four-frame functional confirmation.
