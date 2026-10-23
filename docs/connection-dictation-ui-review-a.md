# Connection and dictation — independent Assessment A

Method: independent design assessment by `/root/dictation_core`. Assessment B and detector findings were not read. This is Assessment A's input to the parent's synthesis, not a claim that the combined critique has finished.

Initial disposition: **Notion connection and publishing choices: SHIP, 32/40. In-app dictation: FIX one P1, 29/40.** The dictation score is below SGL's 30/40 floor until the functional recovery issue below is corrected and confirmed.

## Scope and evidence

Reviewed the frozen source in `src/App.tsx`, `src/components/Settings.tsx`, `src/components/NotionConnection.tsx`, `src/components/NotionDestination.tsx`, and the relevant `src/styles.css` rules. Product and design context came from the Impeccable context script and the incumbent cool porcelain Operate surface. No replacement visual world is proposed.

Visually inspected all twelve `.impeccable/review/connection-dictation-{notion-disconnected,notion-pending,notion-connected,notion-preferences,listening,saved}-{1320,390}.png` captures, including the settled replacement for the desktop disconnected state. These are explicitly illustrative fixtures, not evidence of provider authorization or physical audio capture. Also opened a fresh Chrome tab at `http://127.0.0.1:1420/`, inspected live Settings → Notion and its accessibility tree, and closed that review tab. The in-app browser provider was unavailable; native Chrome computer use supplied live browser inspection. Exact native pending, connected and listening states were assessed from the supplied fixture captures and source.

No detector was run or read by Assessment A. No additional server was started, production authentication invoked, microphone permission requested, personal audio captured, or remote publication attempted.

## Design specificity

The result fits Tuck's existing meeting desk: flat work areas, precise local-capture language, restrained cornflower actions, and editable words as the main output. The connection sequence is organized around a person's workspace and destination rather than OAuth protocol details. Dictation removes meeting recap, tasks and assistant choices from its primary surface. This is a coherent extension of the product's established language.

The main opportunity is behavioral clarity: a voice note must continue to expose the words actually dictated when someone also types or edits text. Additional decoration would not address that problem.

## Initial heuristic scores

All ten Nielsen heuristics apply to these Operate surfaces.

| Heuristic | Notion | Dictation | Assessment |
| --- | ---: | ---: | --- |
| Visibility of system status | 3 | 3 | Connection states and recording/stop states are explicit. |
| Match with the real world | 3 | 3 | Familiar workspace, destination and note language; two copy/behavior mismatches below. |
| User control and freedom | 4 | 3 | Cancel/disconnect/manual migration are clear; dictated source needs a reachable recovery path. |
| Consistency and standards | 4 | 3 | Uses the incumbent controls, tabs, spacing and vocabulary. |
| Error prevention | 3 | 2 | Publishing constraints are useful; the optional live note invites an unexpected result. |
| Recognition rather than recall | 3 | 3 | Destination choices are visible; dictated words vanish from the ordinary stopped view. |
| Flexibility and efficiency | 3 | 3 | Manual connection and editable/copyable text remain available without dominating the flow. |
| Aesthetic and minimalist design | 3 | 4 | Focused workspace, especially the voice-note result. |
| Error recovery | 3 | 2 | Connection retry preserves work; typed dictation currently leaves no visible way back to recognized speech. |
| Help and documentation | 3 | 3 | Concise contextual guidance and honest local/provider boundaries. |
| **Total** | **32/40** | **29/40** | **Good foundation; dictation needs the P1 correction.** |

## What works

- The connection flow progresses from sign-in to destination, with token migration and column mapping disclosed separately. The three publishing-section checkboxes are easy to compare.
- Dictation clearly labels microphone and local transcription, exposes a recognizable stop action, and gives the resulting editable text a quiet, wide reading area. The compact captures show recognized words and Stop dictation without clipping.
- Local-save reassurance, a visible cancellation path, preserved drafts and disabled automatic publishing reduce the anxiety of connecting an external service.

## Priority findings

**[P1] Typing the invited optional note hides the dictated result after stop.** In the initial frozen `src/App.tsx:2421`, recognized speech is shown only while active or processing. The live textarea at line 2427 invites “Add a note while you speak (optional)…”. Core intentionally preserves nonempty user notes instead of replacing them with speech. Therefore, typing a reminder while speaking produces a stopped editor containing only the reminder, while recognized speech disappears. The dictation tab filter at line 2126 also removes the ordinary Transcript tab. Copy text copies only the reminder. The speech remains safe in storage, but the user cannot reach it through the visible dictation flow.

Fix in one bounded functional batch: retain a clearly labeled Original speech disclosure after stopping and provide a way to copy those original words independently from edited notes. Explain that existing typed text is preserved. Keep the normal Copy text action tied to the editable result. Add a behavioral confirmation covering live typing → stop → original words still reachable/copyable, with typed text intact; also confirm original speech after later edits. Suggested follow-up category: Impeccable harden/clarify.

**[P2] Automatic-publishing help names the wrong trigger.** Initial `src/components/Settings.tsx:752` says “After a recap is created,” but core's recording-completion workflow can publish selected notes or tasks even when automatic recap generation is off. This is particularly confusing when only My notes is selected. Describe the actual meeting-finish trigger and selected sections, keeping later Update notes behavior clear. Voice notes remain excluded by the core policy. Suggested follow-up category: Impeccable clarify.

No additional cosmetic changes are required by this assessment.

## Cognitive load and emotional journey

Notion fails one of eight cognitive-load checklist items: five incumbent Settings tabs exceed the four-option guideline, though their existing grouping and stable selection make that a minor inherited issue. The connection itself progresses one decision at a time; manual credentials and mappings stay disclosed. The three publishing choices fit within the guideline.

The ordinary dictation flow has no checklist failures: one start action, a clear stop, then edit/copy. The P1 introduces a working-memory bridge in the edge case: the user must remember speech that was visible during recording but is inaccessible afterward.

The journey is otherwise calm: setup explains what leaves the device, cancellation is explicit, and completion reassures that words are saved locally. The P1 creates the main emotional valley at the moment the user expects to receive their finished note.

## Persona red flags and minor observations

- **Jordan, first-time user:** the phrase “Add a note while you speak” reasonably suggests an additional note, rather than replacing the visible dictated result. This is the P1 trap.
- **Alex, frequent user:** an editable result is useful, but the original speech should remain accessible after edits so correction does not require remembering what recognition first produced.
- **Sam, accessibility user:** the live region announces recognized speech, and controls have labels and ordinary tab semantics. A minor P3 observation is that the entire cumulative live passage is inside the polite status region; a long recording may produce repetitive announcements. This does not require another iteration for this slice.

## Bounded confirmation scope

The initial assessment is complete. The authorized follow-up is one functional correction batch for original-speech access/copy and automatic-publishing copy, followed by one confirmation of the changed flow. Do not reopen the visual design or add cosmetic iterations. Update the dictation score only after that confirmation; retain the initial 29/40 and its reason as provenance.

Questions skipped: the parent task already authorizes the concrete correction and owns the combined Assessment A/B report and disposition.

## One functional confirmation — final Assessment A disposition

**SHIP for the reviewed interface: Notion 32/40; dictation 32/40.** Both changed surfaces meet SGL's 30/40 floor. This completes the authorized single confirmation pass; no further cosmetic iteration is requested.

The P1 is **resolved**. Completed and interrupted voice notes now expose an Original speech disclosure immediately above the editable note. It uses each segment's `originalText` when available, keeps the recognized words distinct from typed or edited notes, and provides Copy original speech. Ordinary Copy text still copies the editable note. The local original does not depend on the original-speech disclosure having been open during capture. No language model or automatic publication is introduced.

The P2 is **resolved**. Settings now describes publishing the selected sections when a meeting finishes, explicitly excludes voice notes, and retains the instruction for later Update notes.

Confirmation evidence was limited to the changed behavior and copy: source inspection, the author's three illustrative `connection-dictation-original-speech-{1320,390}.png` and `connection-dictation-publication-copy-390.png` captures, and an independently run focused Playwright regression, `typing during dictation preserves both notes and original speech after stopping` (**1 passed**). The regression types during live dictation, stops, verifies both sources, exercises clipboard failure/retry, edits final notes, and verifies original speech remains unchanged. It also checks that summary generation and publication were not called. The first restricted test launch could not bind localhost; the authorized retry with localhost/browser permissions passed. These are fixture checks and do not qualify real audio capture or provider sign-in.

Final dictation scores, in the original table's order: **3, 3, 3, 3, 3, 4, 3, 4, 3, 3 = 32/40**. Error prevention, recognition, and recovery each increase by one because typed notes and original words are visibly separated and independently recoverable/copyable. Notion remains **3, 3, 4, 4, 3, 3, 3, 3, 3, 3 = 32/40**; correcting the trigger removes the finding without changing the broader conservative scoring.

The new disclosure is readable in the desktop and compact pixels, and its original-copy action does not crowd the primary editable-note copy action. There are no remaining P0/P1 findings from Assessment A. The previously recorded P3 live-region observation remains optional. Assessment B/detector evidence was still not read during this confirmation.
