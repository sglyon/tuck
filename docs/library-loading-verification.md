# Library loading verification

The goal is to keep startup work proportional to a page of conversations rather than every saved transcript. This is a change inside the Meeting Workspace; SQLite implements the core-owned read operations without a separate read service or model.

## Reproducible acceptance

Build the diagnostic executable, then run the isolated synthetic-library check with Node 22.13 or newer:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --features diagnostics --bin tuck-runtime
node scripts/verify-library-loading.mjs --compare-full
```

The script creates a disposable library through the actual native composition, seeds 1,000 complete meetings and 120,000 synthetic transcript segments, then removes that library after verification. No personal library, microphone/system recording, model download, account sign-in or external provider request is used. `node:sqlite` is a development fixture dependency, not a production runtime.

The following requirements were set before measurement:

- First bootstrap returns 50 headers, under 160,000 serialized bytes, with no note/recap bodies or evidence arrays.
- Cursor traversal returns every meeting once, including creation-time ties, in deterministic order.
- The oldest meeting remains visible to transcript search, exact scopes, global facets and the task inbox even though it is absent from the first page.
- A selected off-page meeting loads its actual complete details. Corrections affect search while preserving original speech; mutations invalidate existing cursors.
- Bounds and malformed cursors fail clearly. An unselected malformed transcript does not prevent lean startup or opening an unrelated meeting.
- The optional same-binary full-bootstrap comparison transfers at least 100 times as many bytes as the lean response.

Raw output is written to `benchmarks/native-workflow/library-loading.json`, including the executable hash, fixture size, transfer bytes and observed timings. Native Rust RSS is sampled after the first response, not at peak. Timing runs share OS caches and are neither cold-start nor video-call benchmarks. WebKit, speech models, local AI and other applications are excluded. A selected meeting still loads its complete transcript; this does not qualify a 16 GB laptop during a real call.

Text search still scans local evidence for literal matches; its work grows with the library even though returned pages are bounded. There is no full-text or semantic index in this increment.

## Result

The final native composition passed all 32 requests on the Apple M6 / 32 GiB development Mac. The fixture held 1,000 meetings and 120,000 segments (69,277,277 bytes of stored transcript documents). The lean response was **27,555 bytes**, compared with **76,224,483 bytes** for the same executable's compatibility full bootstrap: about 2,766 times less transfer. The first selected meeting returned 73,071 bytes on demand.

Rust RSS sampled after each fresh process's first response was **25,296,896 bytes** for lean loading and **380,616,704 bytes** for full loading (about 24.1 versus 363.0 MiB). Launch-to-response observations were 38.7 ms and 3,024.7 ms respectively. These are single-run diagnostic observations with the exclusions above; they are not end-to-end app startup or peak/whole-system measurements. No timing or RSS threshold is asserted by the script.

All page/tie, global scope/facet/task, correction-aware search, cursor-bound/conflict, selected-detail and malformed-unselected-record checks passed. An independent native reviewer inspected the SQL projections, generation rollback, cursor budget and fixture/harness and found no blocking issue. Core contract tests separately exercise both SQLite and memory adapters, including escaped scopes near the cursor budget and old-library defaults.

The comparison uses the same new executable for both APIs. The earlier full response was larger because unedited segments also repeated their text in `originalText`; that duplication has been removed. Edited segments still expose both texts, and raw speech records are unchanged. The old full-bootstrap API remains solely for compatibility and diagnostics; the desktop interface uses paged loading.

Documentation for the development-only fixture API: [Node SQLite](https://nodejs.org/api/sqlite.html).
