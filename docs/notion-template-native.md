# Native Notion meeting template

Tuck provisions a real inline **Tuck meetings** database beneath a page the user explicitly chooses from pages already shared with the connection. It creates an empty, useful destination, not sample meeting rows. Future meeting pages use the existing publication adapter and its consistent managed recap, personal notes, and action-item blocks. The existing content preferences still decide which sections leave the device.

The standard columns are:

| Column | Notion type | Tuck mapping |
| --- | --- | --- |
| Name | title | Meeting title |
| Meeting date | date | Meeting date |
| Folder | select | Folder |
| Tags | multi_select | Tags |
| People | multi_select | Speaker/person labels, never inferred workspace identities |
| Tuck meeting ID | rich_text | Local meeting identifier |
| Added to Notion | created_time | Notion-managed creation timestamp |

On first creation, Tuck attempts to name the initial table **Recent meetings**, sort meeting date descending, show the useful columns in order, and hide the identifier and creation timestamp. It patches the initial view rather than creating extra views. If Notion cannot apply that layout, the verified database and column mapping remain usable, with an explicit setup warning. Reusing a template never rewrites the user's view customizations.

The adapter uses Notion API version `2026-03-11`, `POST /databases` with `initial_data_source.properties`, and the returned data-source identifier for subsequent publication. Database metadata and its data-source schema are fetched and verified before the destination is returned. Browser-generated URLs are ignored in favor of validated UUIDs on the fixed human Notion origin.

## Safety and recovery

The description's first line is the human-readable ownership/version marker `Tuck meeting database · template v1`. Before creation, the adapter reads the selected page's direct children and looks for that exact marker. A matching database must belong to that parent, have one active data source belonging to that database, and preserve all standard column types. A same-named ordinary database is not ownership evidence. A changed owned schema is reported, never repaired silently. Multiple marked templates require choosing an existing database explicitly.

Discovery is bounded to 100 shared page search results; direct-parent inspection is bounded to 500 blocks, ten pagination responses, and 20 child databases. It does not recursively inspect the workspace. The standard HTTP client rejects redirects, disables automatic retries, limits responses to 2 MB, and uses one credential snapshot for the operation. Credentials and meeting content never enter discovery or setup responses.

The core owns the durable setup-intent journal. It writes intent before calling the publication port, persists it in device-local SQLite settings, and exposes only a read-only status projection. Unrelated preference edits preserve it; portable backups exclude it. A replay after interruption or uncertainty passes `allow_create=false`, even after restart. Credential or destination changes invalidate the attempt identity so a delayed response cannot overwrite the newer selection. Successful setup selects the data source and its five mappings atomically while preserving the user’s automatic-publishing preference. No database transaction spans network work.

 `provision(parent, true)` may perform one database creation after its read checks. `provision(parent, false)` only reads and reuses a verified template; an absent template returns `notion_setup_missing`. Transport failures, ambiguous HTTP statuses (including 408 and 5xx), malformed successful responses, and verification failures after creation return `notion_setup_uncertain`. When Notion returned a usable database ID before verification failed, the error preserves its safe Notion URL. A later check can discover the database using the marker without issuing another database POST. This matters because an absent immediately visible result does not prove a previous creation failed. The adapter makes no cross-device exactly-once claim: two independent fresh installations creating simultaneously may still require the user to choose between two templates.

This is a thin translation inside the existing `PublicationPort`, following the established supporting publication capability and its external anticorruption boundary. It adds no separate service, repository abstraction, meeting aggregate, or persistence model.

## Documentation and validation

Context7 is unavailable in this session. The request formats were checked against official current Notion documentation: [Create a database](https://developers.notion.com/reference/create-database), [List views](https://developers.notion.com/reference/list-views), and [Update a view](https://developers.notion.com/reference/update-a-view).

The mock HTTP tests exercise real adapter requests and responses: shared-page filtering and bounds; seven-column creation payload; initial-view sort, visibility, and stable property IDs; read-only template reuse; same-title rejection; changed-schema preservation; ambiguous status recovery without a second database creation; layout-warning fallback; unavailable-parent rejection; and database inspection limits. They do not contact a personal Notion workspace. Live acceptance, if performed separately, is recorded in the main workflow evidence.
