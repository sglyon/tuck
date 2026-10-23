# Portable Rust core

`crates/tuck-core` ports the platform-independent Meeting Workspace from the Python reference into Rust. It does not invoke Python, import a UI framework, open network connections, capture audio, or read the system keychain. The native host supplies those adapters. The existing Python implementation remains an executable reference during the transition.

The public entry point is `MeetingService::handle(self: &Arc<Self>, method: &str, params: serde_json::Value) -> Result<Value, AppError>`. It implements the methods and camelCase DTOs in `contracts.md`, including examples, meeting and speaker editing, bookmarks, capture lifecycle, import/export, streamed chat, hierarchical summaries, publication, model management, settings and account actions. Unsupported method names return `unknown_method`. Unavailable external operations must return an error from their supplied adapter; the core never reports placeholder success.

## Host composition

```rust,ignore
use std::sync::Arc;
use tuck_core::{Adapters, MeetingService};
use tuck_core::ports::{Emit, Store};
use tuck_core::store::sqlite::SqliteStore;

let store: Arc<dyn Store> = Arc::new(SqliteStore::open(data_dir.join("tuck.sqlite3"))?);
let emit: Emit = Arc::new(move |event, payload| {
    // Forward { event, payload } to the existing engine-event UI channel.
});
let core = MeetingService::new(store, Adapters {
    capture, intelligence, publisher, secrets, accounts, archive,
}, emit)?;
// All adapters are Arc<dyn CoreOwnedPort>; see src/ports.rs for exact signatures.
// In Tauri, call blocking adapters and core.handle inside spawn_blocking.
let result = core.handle("bootstrap", serde_json::json!({}))?;
```

All outbound methods are synchronous and require `Send + Sync` adapters. A native host can use a blocking HTTP client and process audio on dedicated workers. Tokio is not a core dependency. Capture retains `OnSegment`, an `Arc<dyn Fn(Value) -> Result<(), AppError> + Send + Sync>`. Callbacks use a weak service reference to avoid a capture/service ownership cycle. Capture must drain callbacks before returning from `stop`; callback errors must stop or surface capture failure, never silently discard evidence. The service emits committed `segment` and `meeting_updated` notifications through `Arc<dyn Fn(&str, Value) + Send + Sync>`.

## Module direction and transaction decisions

`domain.rs` contains meeting lifecycle, publication invariants, transcript validation and preference validation. `ports.rs` owns the adapter and persistence interfaces. `service.rs` composes use cases against those ports. `store/sqlite.rs` imports core types and implements the store. Domain and service never import the concrete store. A single crate keeps deployment and mobile reuse simple; modules preserve the dependency boundary without extra crates or a dependency injection framework.

The `Store::transaction` callback is the transaction boundary chosen by the use case. A callback returning `Ok` commits, and `Err` rolls back. `Repository` methods explicitly save detached meeting aggregates and never commit. The callback cannot escape its borrowed repository. This is a small alternative to a generic unit-of-work hierarchy: one application-facing transaction interface groups atomic metadata/evidence/settings operations and lets SQLite and a memory adapter share the same contract. The interface deliberately includes evidence record operations, rather than pretending segments and bookmarks are independent aggregate roots. This pragmatic transaction facade can be separated into narrower read/write views if a future storage adapter needs that distinction.

Metadata inserts start at version 1; updates/deletes compare the loaded version atomically. Transcript inserts deduplicate by meeting/segment identity and create provisional speaker labels in the same transaction. They do not rewrite meeting metadata or its transcript. Deleting a root cascades its database records. Archive removal is a separate external operation; failure is surfaced after the database delete rather than falsely claiming atomicity across SQLite and the filesystem.

Lifecycle commands serialize independently of segment callbacks. No transaction spans capture stop, inference, account authorization, secret access, publishing, archive cleanup or event emission. Chat persists its user message before inference. Summaries use bounded evidence chunks covering every transcript section, preserve citations and reload current metadata before writing only the generated summary. User notes edited during inference survive. Active AI/publication operations prevent deletion, and duplicate operations of the same kind are rejected.

Publishing commits `syncing` before calling the publisher. The publisher must return `publish_uncertain` for an ambiguous create outcome. Ambiguous network results, invalid returned URLs after successful send, and interrupted `syncing` records become `uncertain` and block automatic retry. Successful publication also blocks another create. Boot recovery changes abandoned recording/paused/processing meetings to `interrupted` without deleting evidence or audio.

## Existing library compatibility

The SQLite adapter opens original version 1 libraries and applies additive migrations: version 2 adds note/recap revisions, and version 3 adds transcript corrections separate from immutable recognized speech. Original meeting/evidence tables and camelCase JSON documents remain readable; new metadata receives defaults. Metadata `version` stays outside the public JSON document. No transcript conversion or destructive migration is required. Schema versions newer than the supported version 3 fail closed; the Python reference is not promised to open upgraded libraries. SQLite enables foreign keys, WAL, a ten-second busy timeout, and Unix database permissions of 0600. The host owns protection of the parent data directory and audio archive.

Use one live service instance per library. Starting a second instance performs crash recovery, so concurrent Python/Rust writers are unsupported even though SQLite CAS protects detached metadata writes. The host must shut down the old backend before selecting the native core. Recovery selects only abandoned active/publication-syncing records. Desktop `bootstrap_library` returns a bounded header page and global overview, while `get_meeting` loads selected details. Full `bootstrap` remains compatible for diagnostics. See [loading contract](library-loading-contract.md).

## Verification

Run `cargo test --manifest-path crates/tuck-core/Cargo.toml`. Contract tests exercise memory and SQLite with the same scenarios: version conflicts, transaction rollback, evidence deduplication, ordering, meeting-scoped records and cascade deletion. Fake-adapter use-case tests cover drain-before-complete, failed start with partial evidence, retained audio on failure, preferences captured at recording start, summaries preserving edits during inference, operation guards, streamed chat, uncertain publishing, bootstrap recovery, example/import/export, settings and secret boundaries. Tests also open a Python-compatible schema/document fixture and verify that newer schema versions are rejected.

The concrete production capture, AI/auth, publication, native archive and secret adapters are host responsibilities. Core correctness tests do not prove their network behavior, hardware support or inference performance. Native engine performance must be measured with the actual selected runtime and model artifacts.

Current rusqlite transaction and connection behavior was checked against the maintained [Connection reference](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html) and [Transaction reference](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html). The dependency is pinned by the crate lockfile to make the native core build reproducible.
