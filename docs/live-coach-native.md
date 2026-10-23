# Native live-agent runtime

The desktop host explicitly starts one wakeable, weak-reference worker that checks the portable service every five seconds. Runtime composition and diagnostic hosts remain inert. The service gates recording, per-call opt-in, new speech, cadence and consent; the worker does not cache transcripts or create model instances.

The native Intelligence adapter serializes manual and background completions through one gate. A background check skips when a request is active or manual work is waiting. Manual work that arrives during an already-started background request waits for that bounded request; it invalidates its result in the service. Recording and speech capture use their existing independent workers.

Automatic prompts are limited to 12,000 UTF-8 bytes including the system prompt and encoded messages. Local Ollama requests use the selected model, a 16,384-token context, two CPU threads, at most 1,200 output tokens, disabled thinking, and the existing two-minute model keep-alive. Hosted Responses requests use the selected model, `store:false`, a 1,200-token output limit and no tools. A global manual web-search preference cannot enable tools for agents. OpenAI's output budget includes reasoning and other non-visible tokens; a model that exhausts that budget fails visibly rather than persisting an incomplete suggestion. See [official token-counting guidance](https://developers.openai.com/api/docs/guides/token-counting).

Background HTTP requests have a 20-second timeout and a 64-KiB raw response bound. Completed answer text is limited to 3,000 characters. Existing local NDJSON and hosted SSE parsers require a completed response; partial, malformed and oversized answers are rejected. ChatGPT authentication can separately refresh with its existing 30-second timeout before the 20-second model request, so 20 seconds is not an end-to-end shutdown guarantee. An already-sent request cannot be unsent: disabling, pausing, stopping or changing the destination invalidates its output and prevents later checks.

Shutdown invalidates service consent and stops/drains hardware capture before joining an outstanding AI request. Idle worker waits wake immediately. This is integrated into quit, explicit shutdown and runtime Drop. The service never restores enabled state from storage.

Initial verification: native adapter tests exercise real loopback HTTP with synthetic text, smaller generation budgets, absent tools, incomplete/oversized responses, UTF-8 input rejection before credentials/network, and serialization/manual priority. Four worker tests cover inert composition, idempotent start, wakeable shutdown, weak ownership and in-flight invalidation. Full native regression at this point passed 116 library tests (two explicitly provisioned fixture tests ignored) and two desktop lifecycle tests; strict all-target Clippy passed.

These are resource bounds and synthetic transport checks. They do not qualify model usefulness, latency or memory use on a 16-GB machine during a video call, or mobile system-audio capture.

## Startup discovered during native acceptance

The first rebuilt package waited on macOS Keychain while bootstrap checked whether a Notion credential existed. A one-second thread-stack sample identified the value-reading `get_password` path; no credential values or personal database were inspected. Local library startup now asks the SecretPort for presence. The macOS adapter performs an exact generic-password service/account query, returns attributes only, excludes data/references, limits results to one and skips items requiring authentication. Actual credential reads, writes and provider authentication retain their existing protection.

ChatGPT startup account projection likewise uses presence metadata rather than decrypting saved tokens. Plan permission remains unknown until an explicit account/provider operation checks it; inaccessible/absent startup metadata is shown as connection needing checking, not proof of sign-out. Other desktop credential adapters retain their prior behavior. This is a macOS startup improvement, not a claim of noninteractive Keychain operations or hardware qualification on every platform.

Additional tests prove core bootstrap uses the presence port even when value reads panic, the macOS query carries no-data/no-reference/no-authentication flags through a fake backend, and startup ChatGPT metadata never reads tokens or asserts plan permission. No real Keychain values are accessed by these tests.
