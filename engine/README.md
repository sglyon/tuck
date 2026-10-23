# Python reference implementation

This directory contains the initial prototype, regression-test oracle and CPU benchmark baseline. The production Tauri app uses the Rust core in `crates/tuck-core`, native adapters in `src-tauri/src/adapters`, and the Apple Swift helper in `native/apple-speech`. Python is neither launched nor packaged by the production build.

Run reference tests with `npm run reference:test` (requires uv). Do not add production features here.
