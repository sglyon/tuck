# Speaker timeline interface

The meeting Transcript view adds a collapsible bottom dock with one colored lane per speaker, timed speech intervals and each speaker’s share of transcribed speech. The transcript keeps its own scroll area. The dock is capped at 34vh on desktop and 36vh at compact widths, with scrolling inside its expanded body. It appears only for meetings with valid timed speech; Notes, voice notes and empty transcripts omit it. Incoming speech extends the timeline without reopening a collapsed dock.

The extension inherits Tuck’s cool porcelain Operate surface, flat seams, system typography and speaker palette. Two tracks use existing darker avatar text shades for contrast. No global token or primitive changes were introduced; `DESIGN.md` and its sidecar remain unchanged. Speaker labels follow the saved names, including renaming. Missing speaker metadata falls back to the identifier’s label; an empty identifier displays Unassigned. The visible disclosure states “Speaker separation is provisional.”

## Timing and navigation

Intervals use transcript start and end times. Blank text, nonfinite timestamps and intervals ending at or before their clamped start are skipped; negative starts clamp to zero. The axis spans the greatest of meeting duration, elapsed recording time and the last valid segment end, so delayed duration updates do not clip new speech.

Each speaker’s overlapping or adjacent intervals merge for drawing and duration accounting. Shares divide that speaker’s merged speech seconds by the sum across all speakers, excluding silence. Speech overlapping across different speakers counts toward each speaker. Lanes sort by speech duration. Shares round to whole percentages, with values below one percent shown as <1%; displayed rounding may not total exactly 100%.

Clicking a lane selects its passage under the pointer, or the nearest passage boundary across silence. The exact segment identifier keeps simultaneous speakers distinct. Left/Right move between that lane’s turns; Home/End select its first/last turn. Navigation clears transcript search, highlights and scrolls to the passage, and turns off live-follow. Accessible lane names, keyboard instructions and selected-passage status accompany the visible controls.

Navigation also seeks saved audio when the user has already loaded it; it never loads audio or starts playback. The playhead follows loaded audio through a local timeline update callback without rerendering the full transcript. With no audio position, it uses the selected passage or zero.

## Verification

Feature validation passed the production build and three model tests in `src/lib/speakerTimeline.test.ts`, covering speech accounting, invalid intervals, unknown labels, lagging duration and nearest-passage selection. Six browser tests in `tests/ui/speaker-timeline.spec.ts` cover exact navigation/search clearing, keyboard turns, collapse, overlapping speakers and renaming, absent/empty states, incoming speech and desktop/390px layout. Six existing library-safety tests also passed during feature work. The final targeted run passed seven tests: all six timeline checks plus the retained-audio test in `tests/ui/library-safety.spec.ts`, which checks paused timeline seeking and audio-to-playhead synchronization.

The final illustrative browser example is captured in `.impeccable/review/speaker-timeline-1320.png` and `.impeccable/review/speaker-timeline-390.png`. The detector ran once, returning 331 advisory preexisting findings and none on the new timeline code or CSS. Independent finish review identified a missing provisional-speaker disclosure; it was added, the captures were refreshed, and the reviewer marked that finding resolved with disposition ship at that fix scope. No new whole-extension score is claimed.

The screenshots establish composition, and the tests establish model and browser contracts. They do not qualify native saved-audio delivery, real capture, speaker identity or diarization accuracy. The displayed meeting is an explicit illustrative example.
