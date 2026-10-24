# Dictation anywhere interface

Global dictation extends Settings → Audio & models, using Tuck’s existing cool porcelain surfaces, cornflower actions, flat seams and system type. The primary instruction is to leave the cursor in another app and use the registered shortcut once to speak, then again to finish. The displayed shortcut comes from native status; it does not promise replacement of Apple’s Fn dictation shortcut.

The interface distinguishes microphone-only local transcription from AI rewriting. Text is copied by the native controller. Automatic paste is described only when native status says it is available, and the result `paste_sent` says “Copied. Paste sent to your text field.” It does not claim the receiving app actually inserted the text. Windows/Linux and unavailable permissions retain the explicit clipboard/manual-paste path.

macOS Accessibility is an optional insertion permission. A user must explicitly choose Open Accessibility settings; viewing status never requests it. Check again refreshes the read-only native status. Recording/copying stay available without Accessibility. Shortcut registration conflicts show the native explanation and retain Start dictation here as a deliberate local action.

A small, persistent status strip in the document pane shows starting/listening/processing/errors without a modal or a focus change. Finish dictation and Save without inserting route to the native coordinator. The global dictation UI delegates capture, clipboard and paste delivery to native code; it does not control another app or run an inference timer. Open voice note requires a user action; incoming shortcut events do not select a different meeting.

The shared UI controller receives native status through App’s existing engine-event subscription. Only the Audio & models section requests an initial status read. Requests coalesce, controls disable while a request is pending, and an older status read cannot replace a newer native event. No new startup status RPC or polling loop is introduced.

## Verification

The strict native-contract fixtures in `tests/ui/global-dictation.spec.ts` passed eight functional tests: no implicit start/permission request; deliberate in-app start and finish; save without copying/inserting; native events preserving the selected document; stale-read ownership; registration/Wayland and permission fallbacks; actionable failure and honest paste-dispatch copy; stable polite state announcements without focus movement. Browser clipboard access is instrumented and remains unused. These fixtures establish UI behavior only.

The single first visual batch passed and produced twelve labeled fixture screenshots: `.impeccable/review/global-dictation-{ready,permission,listening,saved,error,shortcut}-{1320,390}.png`. The one consolidated confirmation is `.impeccable/review/global-dictation-confirm-{ready,permission,listening,saved,error,shortcut}-{1320,390}.png`. It normalizes the compact section heading after button-induced scrolling. Independent Impeccable reviews are recorded separately by the orchestrator. No author score or detector finding is supplied to reviewers.

Real global shortcut registration, microphone capture, focus preservation, secure-field checks, clipboard contents, OS Accessibility behavior and target-app insertion require separate native verification. A 390px browser screenshot does not establish native mobile dictation. No actual audio, external text field, permission prompt or personal note was used to produce this evidence.

## Status continuity correction

The independent finish review requested one material accessibility correction. A single stable, polite, atomic live-region node now lives directly under the app shell, outside pane switches. It announces starting, listening, processing, copied, paste-sent and error states and remains mounted when the visual recording strip disappears. Settings and the visual strip retain their readable labels but no longer duplicate global phase announcements. Errors opening Accessibility settings remain a separate contextual status.

The regression retains the same live-region DOM node throughout all six phases outside Settings, checks unchanged textbox focus, then enters Settings and verifies that the phase announcement still has one owner. Eight functional browser tests and the single consolidated visual confirmation passed. This is a focused DOM/live-region contract check; no claim is made about VoiceOver behavior while Tuck is backgrounded.
