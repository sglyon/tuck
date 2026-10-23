# Native adapter review

Reviewed `src-tauri/src/adapters/{chatgpt,intelligence,notion,secrets}.rs` against the Python reference, runtime contract, maintained library source, and current official provider documentation. The review made targeted corrections without changing the public core ports.

## Corrections

- **OAuth credential concurrency:** sign-in's final credential replacement now shares the refresh lock; account selection also serializes with refresh. A sign-out generation invalidates an earlier browser sign-in, preventing its late completion from silently restoring a signed-out session. Verified subject/client bindings are rechecked against current registration state before saving. The verified identity mapping is persisted before credentials, and a new account becomes active only after credential saving succeeds.
- **Token validation:** malformed scalar/array credential JSON fails safely. New and refreshed token sets require a nonempty access token, valid lifetime, supported token type and matching client ID when supplied. Refresh preserves the latest rotated refresh token, retains existing scopes when the server omits `scope`, and disables plan use when explicit scopes revoke permission. The previously verified ID-token hint is retained instead of substituting an unverified refresh response's ID token.
- **Identity verification:** RS256 signature, issuer, audience, expiration, optional `nbf`, nonce, nonempty subject and authorized-party consistency are enforced. JWT's default 60-second expiry grace is removed. Metadata-derived credential endpoints require the OpenAI HTTPS authority without URL credentials or alternate ports. Callback acceptance requires GET and fresh matching state; empty registration IDs/codes fail closed. HTTP clients disable redirects, proxies and automatic retries.
- **Streaming:** hosted SSE and local NDJSON reject malformed events, invalid text, contradictory terminal status, text after completion, incomplete transport and context exhaustion. Answer limits count Unicode characters; line/event allocations and citation counts are bounded. Outbound message bodies contain only owned role/text fields. Valid web annotations remain explicit HTTPS links with sanitized Markdown labels/URLs.
- **Notion:** renamed title properties are addressed dynamically. Checked `* [X]` items retain checkbox semantics. Request limits include the actual serialized JSON size, covering control-character escaping expansion. Ambiguous POST statuses (including redirects, timeouts and server errors), unreadable accepted responses and unexpected page addresses return `publish_uncertain`; known client rejections remain retryable errors. No create retry is performed. Only summary and personal notes are published.
- **Native secrets:** identifiers are checked before contacting the OS store, including empty ChatGPT account suffixes and control characters. No plaintext fallback is introduced.

The requested asynchronous capture recovery entry point was added to the core as `interrupt_recording(id) -> Result<bool>`. It serializes with lifecycle changes, ignores stale session IDs, drains capture outside a database transaction, commits interruption before clearing the active meeting, retains recovery audio, and then emits durable state. Host wiring invokes it on a separate worker so the capture thread cannot deadlock while stopping itself.

## Verification and limits

`cargo test --offline --manifest-path src-tauri/Cargo.toml --lib`: **42 passed, 1 explicit-weights test ignored**. Tests include a generated disposable RSA/JWKS fixture, signature/claim rejection, refresh rotation and scope changes, account isolation, sign-out cancellation of an earlier sign-in, malformed credentials, PKCE randomness, incomplete/oversized streams, citation rendering, actual Notion JSON size, dynamic properties and uncertain create outcomes.

`cargo test --offline --manifest-path crates/tuck-core/Cargo.toml`: **21 passed**, including asynchronous capture recovery, stale failure notifications and retained evidence/audio.

Strict native Clippy passes with warnings denied after the capture/model owner resolved the reported lints. The core's strict Clippy also passes.

These tests use fake secrets, pure transport translators and generated test credentials. They do not connect a real ChatGPT account, exercise OS keychain prompts, send transcript text to a provider, or create a Notion page. Live OAuth/provider integration remains an explicit user-driven acceptance check.

## Primary references

- [OpenAI registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in): issued client IDs, fresh state/nonce/PKCE, exact callback URI and verified account identity.
- [OpenAI accounts and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions): separate registrations, rotating refresh tokens and revocation.
- [Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events): completion, failure and incomplete terminal events.
- [Notion request limits](https://developers.notion.com/reference/request-limits): rich text/array limits, serialized request cap and uncertain writes.
