# Notion template setup — Assessment A

Method: isolated Assessment A by `/root/notion_template_native`. The reviewer authored the separate native Notion adapter, not this UI. No Assessment B report or detector findings were seen before this assessment was sealed. The parent orchestrator owns the eventual dual-agent synthesis.

Target: `src/components/NotionDestination.tsx` and its rules in `src/styles.css`. Mode: **Operate**, preserving Tuck's existing cool porcelain settings surface. Impeccable's `critique.md`, `operate.md`, `DESIGN.md`, and the existing surface brief informed the review. The target slug resolves to `src-components-notiondestination-tsx`; no ignore list was present. The session context was not rerun.

## Verdict

**31/40 — Good.** The core journey is clear enough for a busy professional: create a meeting database, choose a shared page, confirm its location, and open the finished destination. The interface feels part of Tuck rather than a technical integration wizard. Two nonblocking source issues should be addressed in the single correction batch below. No P0 or P1 defect was found in this scoped review.

| # | Nielsen heuristic | Score | Evidence / limitation |
| --- | --- | --- | --- |
| 1 | Visibility of system status | 3/4 | Loading, selected parent, ready state, and durable uncertain state are explicit; a persisted layout warning is hidden after the completion toast. |
| 2 | Match between system and real world | 4/4 | “Create my meeting database,” the actual parent name, and meeting details use the language of the customer's task. No token, data-source ID, or schema editing is required. |
| 3 | User control and freedom | 3/4 | The user chooses the page and can cancel before creation; existing databases and publishing preferences remain available. Remote creation is deliberately not presented as undoable. |
| 4 | Consistency and standards | 3/4 | Shared buttons, native inputs, disclosures, restrained accent, and flat seams fit existing settings; dynamic focus handoff is not explicitly managed. |
| 5 | Error prevention | 3/4 | A parent is required, actions disable while busy, setup does not upload meetings, and uncertain retry is read-only at the native boundary. Schema-state isolation can be improved. |
| 6 | Recognition rather than recall | 3/4 | Shared pages are visible by name and the template preview explains the outcome; an unresolved attempt does not preserve a visible parent title. |
| 7 | Flexibility and efficiency of use | 3/4 | The default template automaps fields, with existing databases and manual mapping disclosed secondarily. There is no demonstrated dedicated keyboard accelerator, which is acceptable for occasional setup. |
| 8 | Aesthetic and minimalist design | 3/4 | One primary action per step, readable hierarchy, and contained compact layout; the uncertainty warning repeats information. |
| 9 | Error recognition and recovery | 3/4 | Inline error text, “Check for my database,” and existing-database discovery preserve a safe path without another create; some fallback warnings are transient. |
| 10 | Help and documentation | 3/4 | Page-sharing guidance, mapping help, preview, and publication reassurance are contextual; no dedicated troubleshooting entry is shown. |
| **Total** | **All ten heuristics apply** | **31/40** | **Above the user's 30/40 floor.** |

## Evidence

Source and the author's bounded eight illustrative captures were inspected: `.impeccable/review/notion-template-{entry,parent,ready,uncertain}-{1320,390}.png`. They show the entry, chosen page, completed database, and unresolved setup at both widths. The compact parent and uncertainty primary actions are fully reachable in the scrolling content; no horizontal overflow was visible. These fixture captures establish composition and state presentation, not real OAuth, personal-workspace permissions, VoiceOver, or the native Notion API outcome.

This assessment did not create browser tabs, take additional screenshots, inject overlays, start a server, or run the detector. It used the author's shared bounded evidence at the orchestrator's request. No claim is made that a live overlay exists or that the deterministic detector has no findings. Assessment B remains independent.

## What works

- The default path removes configuration work: a named template and page choice replace mapping every column manually.
- Creation is accurately separated from publication. “No meetings are uploaded during setup” and the retained publishing preferences prevent a misleading privacy expectation.
- An uncertain remote operation changes the action to **Check for my database**, and the backend enforces the corresponding read-only behavior. The recovery copy is more useful than a generic retry button.

## Priority findings for one correction batch

1. **P2 — Persist the layout qualification beside the ready destination.** `setup.message` is retained by the core when the database succeeds but view styling fails. The ready card currently shows unconditional success and the warning is only toasted during that session. Show the saved warning beside the ready state so returning users understand which part of setup completed. Suggested command: Impeccable harden/clarify.
2. **P2 — Clear the old schema when switching destinations.** The destination-loading effect does not reset `schema` before a new request. If a new target's schema request fails, the prior target's fields can remain available in manual mapping. Clear the stale schema or bind it to the current destination, and only expose verified current columns. Publishing still validates property types, so this is a recoverable setup annoyance rather than silent content loss. Suggested command: Impeccable harden.
3. **P3 — Shorten repeated uncertainty instructions when next touching this area.** The heading/intro, main error, and follow-up paragraph repeat that the database may exist. Keep a brief status plus one clear check/existing-database instruction. This is polish, not a reason to extend the current verification cycle. Suggested command: Impeccable distill.

## Cognitive load and emotional journey

The entry has a primary creation choice and a secondary existing-database disclosure. The page choice is a familiar list, with two rows in the evidence; search and a bounded scroll container prevent a large flat decision field. The preview is informational rather than seven separate setup decisions. Manual mapping is collapsed by default. The happy path ends with a named destination and an opening link. Uncertainty is an emotional low point, but the named check action and explicit absence of meeting uploads provide a practical recovery path. The repeated warning is the main avoidable reading burden.

## Persona checks

- **Jordan, first-time user:** the selected page name makes the destination concrete; “Map meeting details to database columns” can stay collapsed. A ready-state layout warning should remain visible if that setup step was incomplete.
- **Sam, keyboard/assistive-technology user:** real buttons, inputs, select controls, an `aria-pressed` parent choice, loading `role=status`, and error `role=alert` give useful semantics. No actual native keyboard/VoiceOver trace was performed. The component lacks an explicit focus handoff when the entry/create button is replaced; this is an acceptance limitation, not a proven blocker.
- **Casey, interrupted compact-screen user:** the completed destination and uncertain journal survive app interruption, and compact primary actions remain inside the content width. The app still needs separate native mobile capture and mobile OS acceptance; a 390px fixture is not that qualification.

Questions skipped: the user has already authorized implementation and refinement; this is an internal acceptance assessment, not a new design-direction interview.

## Bounded source confirmation

The author addressed both P2 findings in the permitted correction batch: the ready destination now retains `setup.message` as a status beside its open link, and schema state is cleared on target changes and explicitly bound to `schemaTarget` before mapping controls are rendered. The view API recovery extension adds **Finish database setup** beside the qualified ready state. It reuses the incumbent secondary compact button and wrapping action group, disables while another action runs, labels its own pending state, and reports failure inline without removing the ready destination. The separate native finishing use case cannot create another database or view; ordinary checking remains read-only. These changes close the specific source findings without changing the visual world or adding another configuration decision to the normal completed path.

This confirmation inspected source only; no additional screenshots, live overlays, or detector run were performed. The orchestrator owns actual native finishing acceptance. The conservative **31/40** score is unchanged. The repeated uncertainty copy remains explicitly deferred P3 polish, and native VoiceOver/keyboard acceptance remains unqualified.
