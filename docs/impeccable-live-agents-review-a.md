# Live meeting agents — sealed Assessment A

This is the original independent design assessment by `/root/domain_runtime`, completed before reading Assessment B or detector output. The original scores and findings below are retained as provenance. **Initial disposition: FIX. Live coach: 33/40. Agent settings: 31/40.** Both met SGL's 30/40 floor, but the authored-draft protection issue still required correction.

The same agent subsequently implemented the consolidated corrections at the parent's request. Its implementation and fixture checks are not a second independent verdict. Independent Assessment B and the root agent own confirmation of those corrections and the combined disposition.

## Scope and evidence

Reviewed `src/components/LiveCoach.tsx`, `src/components/MeetingAgents.tsx`, their relevant `src/styles.css` rules, the UI contracts, and the incumbent product/design context loaded by Impeccable. All twelve initial `.impeccable/review/live-coach-{off,listening,suggestions,error,ended,agents}-{1320,390}.png` screenshots were opened and valid. These are marked illustrative fixtures, not live account, speech, model, or hardware evidence.

No detector was run or read by Assessment A. Browser interaction was excluded by the assigned review scope; this assessment used the supplied pixels and source. It does not certify native accessibility behavior, real model quality, inference cost, or 16 GB call-time performance.

## Design specificity

The interface fits Tuck's meeting workspace. Flat seams, readable suggestions, adjacent transcript citations, explicit activation, and the persistent manual composer support the task without distracting from the call. It extends the existing cool porcelain Operate surface; no new visual world or decorative treatment is needed.

## Original heuristic scores

All ten Nielsen heuristics apply.

| Heuristic | Live coach | Agent settings |
| --- | ---: | ---: |
| Visibility of system status | 3 | 3 |
| Match with the real world | 4 | 4 |
| User control and freedom | 3 | 2 |
| Consistency and standards | 4 | 4 |
| Error prevention | 3 | 2 |
| Recognition rather than recall | 3 | 3 |
| Flexibility and efficiency | 3 | 3 |
| Aesthetic and minimalist design | 4 | 4 |
| Error recovery | 3 | 3 |
| Help and documentation | 3 | 3 |
| **Total** | **33/40** | **31/40** |

## What works

- Suggestions retain their agent name and a transcript timestamp, while follow-up saving is an explicit user action rather than an automatic task or message.
- The default two-profile choice and three-open-suggestion cap keep ordinary decisions manageable. Agent editing is inline and uses familiar fields.
- The sticky Turn off action, explicit paused/ended states, and retained manual composer provide a quiet exit and useful continuity.

## Original priority findings

The line references describe the source at the time of the sealed assessment; subsequent correction changed those lines.

1. **[P1] Protect authored agent instructions.** Initial `MeetingAgents.tsx:22` allowed another Edit action to replace an unsaved draft immediately. Leaving Settings also unmounted its local draft. Preserve drafts or require an explicit discard choice before replacing/leaving them. Losing a carefully written 4,000-character monitor is the main emotional valley for experienced users and first-timers.

2. **[P2] Make profile removal recoverable.** Initial `MeetingAgents.tsx:29` immediately deleted the saved profile, with neither confirmation nor Undo. Add one of those protections.

3. **[P2] Keep active monitor names visible.** Initial `LiveCoach.tsx:89` showed only “Listening for useful moments” after activation. Selected names disappeared until a suggestion arrived. A compact active-agent summary would prevent users from having to remember whether Sales coaching was running.

4. **[P2] Announce new advice accessibly.** Initial `LiveCoach.tsx:101` added suggestions without a polite live announcement. Screen-reader users received no indication that advice arrived while focus was elsewhere. Announce a short count or agent name without moving focus or reading the whole card automatically.

5. **[P2] Qualify the manual-chat fallback.** Initial `LiveCoach.tsx:100` always said “You can still ask Tuck below,” including the captured error where the shared local model had stopped. Retain the composer, but distinguish recording continuity from AI availability and provide appropriate reconnect/provider guidance.

A minor observation was that citation and dismissal targets were small in the 390px layout; increasing touch targets belonged in the same bounded correction.

## Cognitive load, emotional journey, and personas

The ordinary two-profile selection and explicit activation have low cognitive load. The coach limits open advice to three items rather than competing with the entire conversation. Custom profiles can increase the visible choice count, but they remain a bounded, user-authored list rather than a collection of unfamiliar settings.

The original active state introduced a recognition gap: someone had to remember which monitors they selected. The main emotional valley was losing carefully authored instructions while switching editors or navigating away. The surrounding interface was otherwise calm, with clear local/hosted boundaries and a visible Turn off action.

- **Alex, frequent user:** retain active monitor names and protect custom instructions across quick navigation.
- **Sam, accessibility-dependent user:** announce newly available advice politely; keep focus in the user's current task.
- **Riley, edge-case tester:** saving failures retained drafts, but changing editors or leaving Settings did not.

## Subsequent correction provenance

At the parent's request, the original A reviewer then authored a single consolidated correction batch in the two components, narrow CSS, and `tests/ui/live-coach.spec.ts`:

- Local session draft restoration and disabled competing Edit actions; explicit Cancel discards.
- Inline removal confirmation.
- Visible active-agent names and a short polite new-tip count.
- Truthful shared-AI availability copy; background failures explain automatic rechecks without a misleading immediate Retry action. Failed foreground actions retain their own retry.
- Compact controls with a 40px minimum target height.

`npm run build` passed. The focused UI suite passed **13 behavior tests**, with its optional capture test skipped during that run. A separate single confirmation invocation passed the capture test and produced fourteen marked fixture images: `.impeccable/review/live-coach-confirm-{off,listening,suggestions,error,ended,agents,agents-removal}-{1320,390}.png`. Every confirmation file was opened and checked for valid content. Initial captures were preserved. These checks establish the implemented UI contracts and composition, not an independent whole-surface approval.

The initial scores remain **33/40** and **31/40**. Independent B/root confirmation must assess the listed corrections before claiming the combined review is complete.

Questions skipped: the parent task already authorizes the concrete corrections and owns the combined Assessment A/B disposition.

## Narrow startup credential review

Independent source review by `/root/domain_runtime`, after the live-agent correction work, covered the separate native startup fix in `src-tauri/src/adapters/secrets.rs`, `src-tauri/src/adapters/chatgpt.rs`, the core ports/bootstrap routes, and the small account-status change in `src/components/Settings.tsx`/`src/lib/types.ts`. The Impeccable craft floor was read for truthfulness and state/recovery copy. This was not another visual assessment, detector run, capture batch, or score increase; the sealed scores above are unchanged.

The macOS presence query is restricted to the generic-password class, Tuck's service, the requested validated account identifier, and at most one result. It requests attributes without credential data or references and skips items requiring authentication. Inspection of the installed `security-framework` 3.7.0 implementation confirms that the skip option maps to the OS authentication-UI-skip parameter. Ordinary credential reads/writes/deletion and the explicit account/inference authorization paths remain separate. Non-macOS presence behavior retains the legacy read path; this review does not claim that all platforms now have a noninteractive metadata check.

One material semantics issue was raised and corrected: a negative result from an authentication-skipping query cannot prove that a protected credential does not exist. Startup account metadata now maps both a negative presence result and an unavailable result to `connected: null`, rather than labeling those saved registrations Signed out. Discoverable presence can report `connected: true`; startup always leaves `sharing: null`, because metadata is not proof of a current plan grant. The corresponding test now expects null for the negative-presence profile. Inactive accounts with unknown connection state retain a Use action, allowing the explicit account path to check credentials instead of hiding them.

The UI distinguishes Connection needs checking from explicit Signed out, and unknown sharing from Plan usage enabled. No token contents enter the DTO, source-level metadata tests use fakes, and no credentials or OS authentication were accessed during this review. **No remaining material finding in this narrow source/copy scope.** The root agent owns focused regression/build execution and actual native startup/control acceptance; source review alone does not establish that an OS service can never block.
