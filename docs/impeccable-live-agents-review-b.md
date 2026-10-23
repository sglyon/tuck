# Live meeting agents — independent Impeccable assessment B

Method: independent child-agent assessment B (`/root/live_coach_core`), isolated from assessment A. The reviewer authored core behavior and did not author the interface. No A findings or scores were read. This is B’s sealed first-pass report for the orchestrator’s combined critique, not a second combined critique.

Scope: `src/components/LiveCoach.tsx`, `src/components/MeetingAgents.tsx`, their App integration and CSS, and twelve first-pass fixture screenshots: off, listening, suggestions, error, ended and agents, each at 1320px and 390px. Mode: Operate. The existing Tuck visual world remains the authority.

## Design health

| Heuristic | Score / 4 | Evidence |
| --- | ---: | --- |
| Visibility of system status | 3 | Off, listening, paused, reviewing, failure and ended states are explicit; retry acknowledgment remains weak. |
| Match between system and real world | 4 | “Ask next,” “Follow-up,” “Turn on for this call” and agent focus/pace labels use understandable working language. |
| User control and freedom | 3 | Per-call opt-in, persistent Turn off, Dismiss, Cancel and explicit Save are strong; discarded advice and switched unsaved drafts lack recovery. |
| Consistency and standards | 4 | The assistant’s cornflower controls, cool porcelain surface, typography, flat seams and native form controls fit Tuck’s incumbent theme throughout. |
| Error prevention | 2 | Hosted consent and disabled states prevent accidental sharing; another profile’s Edit can silently replace an unsaved draft. |
| Recognition rather than recall | 3 | Profile labels and cited tip sources are visible; enabled mode could identify which selected monitors remain active. |
| Flexibility and efficiency | 3 | Editable goals, multiple monitors, clear defaults and per-call selection support both quick and specialized use. |
| Aesthetic and minimalist design | 3 | Quiet bounded cards and restrained color work; the existing large manual-chat welcome still competes with live tips on short screens. |
| Error recovery | 3 | Draft preservation on save failure and inline retries are good; retry can leave the exact same state during the core backoff window. |
| Help and documentation | 3 | Inline sharing, pace and action-limit explanations are helpful; source quotations and screen-reader arrival announcements could be clearer. |
| **Total** | **31 / 40** | **Meets the requested floor. Material fixes below remain worthwhile.** |

## Design specificity

The result belongs to Tuck. Optional, timestamped help sits close to the transcript and leaves manual questions available. The UI avoids turning a busy professional’s call into an autonomous-agent control panel. Its ordinary controls, small type hierarchy and clear per-call boundary serve this product better than decorative cards or a new visual system.

All twelve supplied screenshots were visually inspected. Both widths retain the primary controls, advice and composer; no clipping or horizontal overflow was apparent in those fixtures. The settings form remains legible at 390px with full-width text fields and a plain pace selector. These screenshots are explicitly illustrative and do not establish native runtime behavior or model quality.

## Strengths

1. Automatic hosted sharing is explained at the enable action, and a new call starts off. This puts the decision in the right place.
2. Advice has an agent label, transcript timestamp and separate dismissal. Follow-ups become tasks only through an explicit action; save feedback points to Tasks and calls out owner/date review.
3. Quiet failure handling preserves the call’s recording context and the existing manual composer. Paused and ended states are plain and do not offer misleading record-independent enable actions.

## Priority findings

### P2 — Switching editors can silently discard unsaved agent instructions

`MeetingAgents.tsx` keeps every profile’s Edit button enabled while a draft is open. Each click immediately calls `setDraft({...agent})`, replacing any typed name or instructions without warning. A user adjusting two monitors can lose several paragraphs of goals by selecting the next profile.

Disable other Edit controls while editing, or explicitly guard a dirty switch. Keep Save and Cancel as the obvious exit. A small interaction test should verify that a second Edit cannot replace dirty input.

### P2 — “Try again” does not acknowledge a deferred retry

When the provider is unavailable, Try again re-enables the current selection. The core deliberately preserves cadence and backoff, so the returned DTO may contain exactly the same unavailable status/message and no request will run until the existing due time. The interface gives no confirmation that the click had an effect.

Preserve the resource limits and add honest feedback such as “Live help will check again shortly.” Explain that recording continues. Avoid promising that manual AI answers will succeed while the same local provider is down; the composer can remain available without making that claim.

### P2 — Incoming tips are not announced to assistive technology

The new list and status text update inside a named section, but the component does not provide a polite live announcement for newly arrived tips. A screen-reader user focused on the conversation or composer can miss the feature’s primary output entirely.

Add a short polite announcement for a newly arrived suggestion or count. Do not mark the entire live transcript/card list as assertive, move focus, or repeatedly read old suggestions.

### P2 — Mobile dismiss target is only 30px

`.live-coach-source .icon-button` fixes dismissal to 30px square. The 390px fixture is readable, but that target is unnecessarily precise during a call. Increase the hit area toward 44px at touch layouts while keeping the icon visually small. Native mobile capture remains out of this feature’s delivered scope.

## Cognitive load and personas

The default call has two named monitors and one enable action. Editing goals is appropriately deferred to Settings. No high-load decision point appears in the default first viewport. Up to eight custom choices can add density, but explicit names and the three-per-call limit keep the task understandable.

- Busy meeting host: the controls stay quiet and avoid interrupting the conversation. A non-acknowledged retry is the main “did that work?” moment.
- First-time user: quiet and sales defaults provide a useful start; explicit sharing copy makes hosted behavior understandable. Large model-availability problems should say recording continues rather than imply manual AI is working.
- Screen-reader or touch user: controls have useful labels and standard form semantics, but new suggestions need a polite arrival announcement and dismissal needs a larger touch target.

## Detector and browser evidence

The bundled targeted detector was invoked once with the two component paths and returned `[]`. The CLI’s single-target behavior makes `LiveCoach.tsx` the deterministic scan scope; `MeetingAgents.tsx`, CSS and integration were manually reviewed. This is a targeted clean result, not a claim that the entire application has no detector findings.

A fresh Chrome review tab was requested at the observed development URL `http://127.0.0.1:1420`; the browser returned `net::ERR_CONNECTION_REFUSED`. Available CUA APIs exposed native input, accessibility and screenshots, without a mutable DOM/script injection API. No overlay was injected or represented as visible. No review-only server was started. The twelve supplied desktop/mobile screenshots plus source served as the explicit fallback evidence.

The per-session Impeccable context setup was already performed in the parent workflow and was not rerun. The loaded skill, critique, Operate and relevant native audit guidance informed this assessment. No interface edits or additional screenshot rounds were made by B. The detector’s temporary JSON output is disposable and contains only `[]`.

Questions skipped: this is a delegated implementation review; the orchestrator consolidates material fixes and owns the user-facing conclusion. The user already authorized implementation and delegation.

## B confirmation after the consolidated material fixes

This is an appended confirmation; the sealed 31/40 first-pass assessment above is unchanged. B independently read the updated component/CSS source and inspected all fourteen supplied confirmation fixtures, `.impeccable/review/live-coach-confirm-{off,listening,suggestions,error,ended,agents,agents-removal}-{1320,390}.png`. No additional screenshots, detector runs, browser sessions or UI edits were made for this confirmation.

Accepted fixes:

- Profile editors now block switching while a draft exists. Session storage plus an in-memory fallback preserves navigation drafts, and the inline explanation states how Save and Cancel finish editing.
- Removing a saved profile requires a separate inline confirmation naming the agent, with a Keep agent exit. This adds an appropriate decision without interrupting ordinary editing with a modal.
- The unavailable state explains automatic checks and that manual chat uses the same AI connection. It offers AI settings rather than a misleading immediate background retry. Recording, paused and ended copy remain truthful.
- Active agent names are visible beneath the live status. A short polite live announcement reports new suggestions without shifting focus or rereading the full transcript.
- Compact layouts increase tip actions, citations and dismissal hit areas to at least 40px. This is a material improvement over 30px. A future shipped native mobile interface should still meet that platform’s full 44pt/48dp targets; this webview confirmation does not certify native mobile conformance.

The screenshots retain Tuck’s existing quiet theme, readable controls and usable single-pane layout at 390px. The larger error explanation can place the lower card action below the live section’s scroll boundary; the bounded scrolling region and persistent heading/composer keep it reachable without an additional blocking defect.

| Heuristic | Confirmed score / 4 |
| --- | ---: |
| Visibility of system status | 3 |
| Match between system and real world | 4 |
| User control and freedom | 3 |
| Consistency and standards | 4 |
| Error prevention | 3 |
| Recognition rather than recall | 4 |
| Flexibility and efficiency | 3 |
| Aesthetic and minimalist design | 3 |
| Error recovery | 3 |
| Help and documentation | 3 |
| **Total** | **33 / 40** |

The score increases for prevention of destructive editing mistakes and recognition of active monitors. The remaining first-pass observations are either addressed or bounded non-blocking details; no further polish loop is requested. The requested 30/40 floor remains satisfied.

B also independently checked the revised native shutdown order in `src-tauri/src/runtime.rs`: explicit shutdown/quit and Drop now invoke core shutdown first, which invalidates coaching and stops/drains capture, before joining the coaching worker and stopping reminders. An already-issued provider completion can still delay process exit, but it no longer delays the stop of microphone/system recording. This confirmation is a source review; native runtime tests remain the orchestrator’s separate evidence. Hosted timing must continue to distinguish the potentially 30-second credential refresh from the separate 20-second completion HTTP timeout.

The failed first-pass Chrome review tab was explicitly closed. No review-only server remains. Questions skipped: the implementation and bounded confirmation are already authorized, and the orchestrator owns final acceptance.
