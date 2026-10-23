# Tuck architecture

## Problem space: where the investment goes

Tuck competes on helping a person stay present in a conversation while retaining trustworthy, private evidence and getting useful help immediately.

| Subdomain | Classification | Why |
| --- | --- | --- |
| Meeting understanding | Core | Contextual answers tied to what was actually said, useful notes, and explicit evidence can make this better than a recorder plus a generic chatbot. |
| Private meeting capture | Supporting | Correct microphone/system capture, recoverable recording, and speaker labels are necessary to deliver the experience; OS audio plumbing itself is not the competitive moat. |
| Notes publishing | Supporting | Moving the result into a user's chosen workflow matters, while Notion's page model should not shape the meeting model. |
| Model inference, identity, credentials, storage | Generic | Existing runtimes, OAuth standards, OS keyrings, and databases solve these problems. Correctness matters; inventing replacements does not differentiate Tuck. |

Problem-space statement: the core is trustworthy meeting understanding, supported by private capture and workflow publishing, using standard inference, identity, credential and persistence mechanisms.

## Solution space: one context, one application

The **Meeting Workspace** is one bounded context owned by the Tuck team. The repository started empty, the language is unified, and there is no evidence of independent teams or conflicting models that would justify splitting it further. Capture, AI, and publishing modules are responsibilities inside this application, not microservices or pretend bounded contexts.

External systems have divergent terms: an OAuth registration is not a meeting participant; a Notion page is not a meeting; an ASR stream is not a transcript. Their adapters translate these models at the edge. OpenAI, Notion, and inference-runtime maintainers are independent upstream suppliers whose APIs Tuck cannot control; their schemas stay in anticorruption adapters. Tuck owns the vocabulary and DTOs in `contracts.md`.

The app is a single-user desktop process. The trusted Rust host is the inbound security boundary; a narrow native Swift helper handles Apple Core ML inference. The desktop exposes no public HTTP server and introduces no event bus, DI framework, event sourcing or CQRS subsystem. The separate Cloudflare authorization Worker is described below.

## Language and state

- **Meeting**: the user's conversation workspace. It owns its title, notes, summary, lifecycle, and publication state. A defaulted immutable kind distinguishes meeting work from microphone-only dictation while reusing the same lifecycle and evidence.
- **Transcript segment**: immutable speech evidence with a source, provisional speaker, and audio time range. User corrections are separate records; the interface and derived context identify the effective corrected text and retain the original. Transcript text is evidence, never executable instructions.
- **Speaker label**: a meeting-scoped name assigned by the user. A diarization cluster is not a verified identity.
- **My notes**: user-authored text. Generating a summary never replaces it.
- **Summary**: an editable recap, optionally generated from a transcript snapshot with timestamp citations. It is interpretation, separate from speech evidence and personal notes.
- **Bookmark**: a user's marker at an audio timestamp.
- **Publication**: a snapshot of notes/recap/actions sent to the chosen Notion destination. Known pages support explicit updates; an uncertain creation outcome requires verified recovery instead of a blind retry.
- **Recording**: draft → recording ↔ paused → processing → complete. Startup changes abandoned active states to interrupted. A pristine failed start returns to draft; partial evidence stays recoverable.

The small `Meeting` aggregate protects lifecycle and publication invariants and carries an optimistic version. Segment evidence is an append-only stream keyed by meeting and segment IDs; append does not rewrite or load the entire transcript. Names, bookmarks, and messages are explicit meeting-scoped records. The application composes them into read DTOs. Root deletion cascades local records in one transaction. Audio is managed through its separate archive port because filesystem deletion is not a database transaction; a failed audio cleanup is surfaced, not silently claimed as complete.

## Ports and adapters

The concrete reason for hexagonal architecture is SGL's requirement to replace SQLite with hosted persistence later and to reuse the workflow for mobile. The portable Rust crate `crates/tuck-core` owns the domain model, application workflows and outbound traits. It has no Tauri, audio SDK, HTTP client or Python dependency. Concrete native adapters live in `src-tauri/src/adapters`; `src-tauri/src/runtime.rs` wires them once.

| Core-owned port | Current adapter | Responsibility |
| --- | --- | --- |
| Repository | SQLite or in-memory transaction view | Explicit metadata saves with version checks, append-only evidence, speaker names, bookmarks, messages and non-secret settings. |
| Store | SQLite or in-memory transaction boundary | Commit only when the application callback succeeds; repositories never commit. |
| RecordingPort | CaptureEngine | Capture and local inference, returning domain-shaped segment evidence. |
| IntelligencePort | Intelligence | Local or authorized hosted AI, bounded context, streamed answers. |
| AccountPort / SecretPort | ChatGPTAuth / OS keyring | Verified account metadata and native protected credentials. |
| PublicationPort | NotionPublisher | Translate notes into Notion and distinguish uncertain network outcomes. |
| AudioArchivePort | LocalAudioArchive | Resolve owned audio, copy recovery recordings, remove owned files, write exports and stage portable backup/restore. |

Tauri commands call `MeetingService` on bounded blocking workers, keeping model and network work away from the webview. A diagnostic `tuck-runtime` JSONL executable calls the same composition root for integration checks. The former Python implementation under `engine/` remains a reference test oracle and benchmark baseline; it is not called or bundled by the production desktop app. Business rules live in the Rust core, not RPC dispatch. Publication selection projects only chosen text and allow-listed metadata before the publisher sees it. Template provisioning uses that same publication port; a small device-local setup journal brackets remote creation and prevents blind retries after interruption. It is not portable meeting data and cannot be changed through user preferences. Notion OAuth remains an adapter: an authorization-only Cloudflare Worker protects the shared app secret, while one short-lived Durable Object coordinates each handoff. This does not add a hosted meeting database or account system. The desktop stores its own credential bundle in the OS keyring and a per-user OS file lease prevents competing processes from rotating it concurrently. See [Rust core](rust-core.md) for exact port and transaction contracts.

## Transactions and concurrency

SQLite uses WAL, foreign keys, schema version 4 and bounded busy timeouts. The v2 migration adds lazy note/recap revision history; v3 adds separate transcript corrections without rewriting raw evidence; v4 adds bounded device-local calendar snapshots, reminder state and an occurrence lookup index. Older meeting metadata receives defaults. Each short use-case mutation invokes the store transaction, delegates to domain methods, commits on success, then emits a UI notification. A stale metadata aggregate cannot silently overwrite a newer version. Segment IDs deduplicate retransmission.

No database transaction is held during capture shutdown, inference, OAuth, or publication. AI reads a snapshot and reloads the current aggregate before saving a summary, so notes edited while the request runs are preserved. Recording lifecycle operations serialize separately from segment persistence; capture can drain callbacks while stop waits. A meeting with active recording or AI work cannot be deleted. Remote requests happen outside transactions, with local durable state bracketing them.

Portable backups take a consistent metadata/evidence snapshot, then stream owned audio through the archive adapter outside the database transaction. Restore validates the full manifest and stages audio before committing new meeting IDs; existing IDs are skipped as a unit. Audio installation follows that commit and reports partial failures truthfully. Shutdown waits on the same lifecycle guard as backup/restore. These rules do not pretend a filesystem and database share one atomic transaction.

Desktop startup returns a bounded page of meeting headers and global scalar counts/capped facets. It loads a complete meeting only when selected, with a bounded cache in the interface. Core-owned read operations project headers and page by stable keys directly in SQLite; a transactional generation invalidates stale page chains. No separate read service or write model is introduced. Calendar preparation atomically reuses a workspace by occurrence identity. A host-owned low-frequency reminder worker uses the core eligibility policy and durable delivery claims; the cached calendar read never calls providers or credential stores. Calendar cache and reminder state stay out of portable backups. See [meeting-day contract](meeting-day-contract.md). Full bootstrap remains a diagnostic compatibility command. A single selected meeting still loads its complete transcript. See [loading contract](library-loading-contract.md) and [verification](library-loading-verification.md).

## Replacing SQLite later

Implement the existing core-owned `Store` and `Repository` ports in a hosted adapter, then select that adapter at the composition root. Use stable UUID meeting/segment IDs, durable transcript ordering, atomic metadata version checks, idempotent segment insertion, and atomic deletion. Run the repository contract tests against that implementation.

A hosted/realtime product also requires actual product decisions that swapping SQL cannot solve: account ownership, row-level authorization, offline conflict resolution, encryption and retention, partial uploads, and whether audio ever leaves the device. Those are deliberately unimplemented. No Supabase SDK, schema, or background sync is added until those choices exist. Realtime notifications can invalidate the same DTOs after committed writes; the domain's lifecycle and notes rules remain unchanged.

## Decisions and limits

- Keep a modular single application. Splitting deployment units would add coordination without a model need.
- Persistence orientation uses explicit `save`; SQLite does not implicitly track changed objects.
- Store actual audio only in the local audio directory. Credentials never enter SQLite or public DTOs.
- Durable storage precedes UI events. Events are transient UI notifications, not a message-delivery system.
- Model and audio work are isolated from the webview, with bounded queues and thread counts in the capture adapter.
- A future cloud adapter is a possibility, not a claim that offline multi-user synchronization is solved.

Pattern-selection route used: `ddd-subdomains` → `ddd-bounded-context` → `ddd-hexagonal-architecture` → `ddd-aggregate` → `ddd-repository` → `ddd-application-services`. These are proportional to the actual requirements; no generic base repositories or framework container are needed.
