# Meeting workspace

Mode: Operate. Target: src/App.tsx, src/components/Settings.tsx, src/components/TranscriptTools.tsx, src/components/BackupRestore.tsx, src/styles.css.

A busy professional needs to start a private meeting capture, retain their own thoughts, recover exact spoken evidence, and ask a question without leaving the call. The library/document/assistant topology keeps those actions visible without a dashboard.

The visual direction is the cool porcelain listening desk (seed e4e0329f): system typography, precise flat seams, cornflower active controls. The signature interaction is a recap or assistant citation returning to a highlighted transcript moment.

First launch is empty; the example is an explicit user action. All privacy and provider states must be truthful. Setup details disclose progressively. Small screens show one pane with bottom navigation; native mobile audio remains a separate platform phase.

Evidence: screenshots in .impeccable/review, browser tests in tests/ui/workspace.spec.ts, independent Impeccable report owned by the root agent. No image comp was used; the user chose direct code design.

The safety extension keeps source evidence recoverable. Correcting words is an explicit inline edit with the original retained; conflicts keep the draft available. A scoped saved-audio player uses native audio controls and never autoplays. Recovery creates a separate meeting rather than replacing the original. Privacy holds portable backup/merge restore with chosen files, a count preview, optional audio and truthful partial-audio reports. Editor flushing applies before file operations, hide and quit.

Workflow and safety confirmations are recorded in `docs/workflow-ui-review.md` and `docs/safety-ui-review.md`; each reviewed area meets the 30/40 floor. Captures labeled fixtures are interface contracts, not personal-service, custom-protocol or hardware qualification.

Large-library loading preserves this world: one header page, selected details on demand, quiet loading/retry and explicit Load more. Counts/facets/search/task scope cover the full library. Expanded filters scroll independently on compact screens so results and recovery actions retain usable space. Background speech does not collapse older loaded rows. Loading review evidence uses explicit illustrative fixtures and does not establish WebKit RAM or call-time hardware performance.

Meeting-day continuity extends the same Operate surface. Next meeting has a persistent, compact title/time/action in the library; Calendar & reminders discloses connection, refresh, imported-snapshot age and the optional five-minute preference. The document uses a flat cornflower wash for a due reminder with prepare/open and dismiss. It never steals focus, starts capture or joins a call. Preparing flushes pending editors and atomically reuses existing notes before navigation. Active recording/paused/processing suppresses interruption; ended reminders expire locally even when an RPC fails. Native notification denial and unavailable delivery preserve the in-app path.

Meeting-day evidence is the illustrative desktop and 390px fixture batch in `.impeccable/review/meeting-day-*.png`, the strict browser ports and workflows in `tests/ui/meeting-day.spec.ts`, and the implementation/validation record in `docs/meeting-day-ui.md`. No personal calendars, audio devices, notification permission or connected accounts were used to produce these captures. A browser resize repaint artifact was replaced in the single bounded confirmation; the final captures were opened and validated.
