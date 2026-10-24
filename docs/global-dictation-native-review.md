# Native global dictation review

Reviewed 2026-10-24, independently of the controller and platform adapter authors. This is a source review of `global_dictation.rs`, its `runtime.rs`/`main.rs` integration, and `adapters/dictation_target{,/macos,/unsupported}.rs`. No microphone, real clipboard, active field, accessibility permission, or credentials were accessed. The reviewer did not edit production code; during confirmation the reviewer added one fake-only interruption regression test and its test hook.

## Material findings at the first reviewed snapshot

1. **P1 — Cancel can precede a prepared start without cancelling it.** `prepare_target()` captures identity and releases the operation mutex before `dispatch_dictation()` starts a worker. `toggle()` obtains its generation only when that worker runs. Therefore `prepare_target → cancel → toggle(prepared_target)` starts a fresh recording after Cancel; the old preparation adopts the new generation. Bind the prepared action to the generation observed before yielding, and reject/discard obsolete actions. Cover this exact sequence with a deterministic fake test. Closing already prevents the equivalent shutdown sequence.

2. **P2 — An interruption during start can be missed by ownership observation.** `begin()` assigns `State.session` after `app.start()` returns. A terminal `meeting_updated` emitted after native capture starts but before that assignment sees no owned session; the controller then announces Listening for an already-interrupted voice note. Establish ownership while starting, remove it on failed starts, and only announce Listening if that ownership survived. Test a fake start that emits an interruption immediately before returning success.

3. **P2 — Global readiness can inspect an old microphone selection.** `NativeApplication::ready()` calls `readiness_for_dictation()` without configuring the capture adapter from current service settings. The existing runtime readiness route and actual start both configure first. A changed device preference can consequently leave the global preflight rejecting the previous unavailable device even though a valid replacement is selected. Configure current dictation settings before this metadata-only check.

The implementation author was notified of all three findings before this document was saved. These are initial findings, not claims that later revisions retain them.

## Confirmation and disposition

All three findings are **resolved in the subsequently reviewed source**:

1. `PreparedToggle` carries the generation captured before native target capture and a single-use reserved ticket. Another preparation is rejected while the ticket is reserved; cancellation abandons the reservation and discards its target, and a stale worker fails the generation/ticket check before starting. Native thread-spawn failure also abandons preparation. The deterministic `prepared_actions_are_single_use_and_cannot_start_after_cancel` test covers the original sequence.
2. Session ownership is established before calling `app.start()`. Failed starts remove that ownership; after success the controller checks active capture and only publishes Listening while it still owns the session. The reviewer-added `interruption_emitted_during_start_retires_ownership_before_listening` regression injects a terminal observer event synchronously inside fake start, following core event-before-active-clear ordering. It verifies Idle with no ownership, no stop of another capture, and zero clipboard delivery.
3. `NativeApplication::ready()` now loads current service settings, explicitly disables diarization for dictation, configures capture, and then checks microphone/model readiness.

Reviewer-run confirmation: `cargo test --manifest-path src-tauri/Cargo.toml global_dictation::tests --lib` — **9 passed**, using fakes only. `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` also passed after the new regression test. The parent separately reported the full native regression: **135 library tests and 3 binary tests passed; 2 ignored**, including the new interruption test. No remaining finding from this three-item confirmation.

## Verified safeguards

- Native shortcut routing compares shortcut identities. Only the dictation chord uses the Pressed/Released latch, and its dispatch does not call the window-show/focus intent.
- Target capture happens on the native callback before its worker is spawned. Capture and validation inspect application/AX identity and field metadata; they do not fetch field contents, selections, window titles, or old clipboard data.
- macOS requires an enabled, focused, writable text role, checks secure event input, rejects secure/unknown field policy and named terminal/password-manager apps, and rechecks PID, bundle and AX-element identity before sending Paste. It never activates another app or sends Enter.
- The paste path waits at most 500 ms for physical shortcut modifiers to clear, checks cancellation during that wait, and validates focus again afterwards. AX calls use a short messaging timeout rather than unbounded polling.
- Clipboard writing follows successful core stop and complete dictation validation. Empty, incomplete, failed and oversized transcription results do not reach delivery. Original recognized segments, rather than editable notes, provide the output.
- Controller cancellation increments generation before waiting for its operation mutex. Explicit runtime shutdown marks closing and cancels delivery before core capture shutdown. Delivery checks permission to proceed before writing and again before Paste; an already-completed clipboard write cannot be undone by a later cancel without reading/restoring private clipboard data, which this implementation deliberately never does.
- Delivery retains one target and one clipboard writer. Plain-text length/control-character checks bound delivery; the output is not executed or sent to a shell. Windows/Linux expose clipboard-only behavior and report automatic paste as unsupported.
- Outcome text distinguishes a sent Paste shortcut from verified insertion. Changed/unavailable/protected targets fall back to copying successful text, rather than choosing a replacement field.

## Validation limits

This review does not establish real application compatibility or successful OS event delivery. Parent-reported platform policy tests and Clippy results are separate validation evidence; the reviewer reran only the fake host-controller tests and formatting check listed above. Native acceptance should still exercise a normal text field, changed focus, denied Accessibility, a secure field, and modifier-held fallback with explicit test content.
