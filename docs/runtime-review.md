# Runtime review

> This review covered the Python reference implementation. The current Rust production adapters and additional fixes are reviewed in [native adapter review](rust-adapter-review.md); current integration evidence is in [native verification](native-verification.md).

Independent review checked the domain/application dependency direction, repositories, capture lifecycle, OAuth, provider streams and Notion failure handling on October 23, 2026.

## Findings corrected

- **P2 — Account identity after retry:** reauthorization used a detached registration copy. A successful retry now updates the canonical profile so verified identity persists, and a later different subject cannot overwrite that account's credentials. Regression tests exercise both outcomes.
- **P2 — Capture startup callback race:** a source could report failure during startup without throwing, then capture would return success. Startup and resume now serialize acceptance against failure callbacks and reject the failed source. Tests reproduce both races.
- **P2 — Audio retention before final commit:** the capture adapter could remove audio before the application durably marked the meeting complete. Retention is now applied through the existing audio archive port after completion commits. A simulated final persistence failure preserves the recording for recovery. Shutdown preserves interrupted recordings.

No additional architecture layer was added to fix these issues.

## Architecture verdict

Pattern: ddd-hexagonal-architecture  
Verdict: CONFORMS  
Findings: none  
Not checked: actual hardware behavior; unavailable hosted persistence implementation.

Pattern: ddd-repository  
Verdict: CONFORMS  
Findings: none  
Not checked: unavailable hosted adapter.

Core-owned ports, one composition root, explicit transactional saves, optimistic versions and shared in-memory/SQLite contract tests support the requested store boundary. No additional abstractions were recommended. The native bridge compiles on this macOS host; Windows and Linux hardware/native execution remain unverified.

Hosted providers and Notion were exercised against deterministic HTTP fakes. OAuth tests verify real signature/audience/nonce validation against a generated test key and a fake loopback authorization callback. No real user sign-in, account consumption, Notion publication or microphone/system recording was performed.
