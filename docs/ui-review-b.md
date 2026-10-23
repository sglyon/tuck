# UI Assessment B — detector and browser evidence

Method: independent subagent `/root/impeccable_evidence`; Assessment A findings were not consulted. Root confirmed A had completed before B findings were communicated. No UI source edits were made by this agent.

## Scope and limits

Sources: `src/App.tsx`, `src/components/Settings.tsx`, `src/components/Markdown.tsx`, `src/styles.css`; live preview `http://127.0.0.1:1420`. The UI implementation agent was editing during the pass, and Vite HMR updated the page. Treat the initial pass and confirmation as time-scoped evidence rather than one frozen baseline. This was one batched browser pass plus one confirmation pass, with no further polishing loops.

Native browser fallback: `cua.createBrowserTab("iab", ...)` reported browser unavailable, then `cua.listBrowsers()` returned `[]`. Therefore an isolated headless Chrome browser/new page through installed Playwright was used. Sandbox launch failed with SIGABRT; the approved escalation succeeded. Browser contexts were closed. No real user browser storage was touched. No recording, microphone grant, account authorization, or credential entry was attempted. Hardware/AI unavailability in browser preview is intentional and explicitly labeled in-product.

## CLI detector

`node /Users/sglyon/.agents/skills/impeccable/scripts/detect.mjs --json src` ran exactly once. Its complete JSON is `[]`: **0 findings; 0 rules; no file locations and no CLI false positives**. Raw JSON retained at `.impeccable/review/b-cli.json`. The ignore list `.impeccable/critique/ignore.md` was absent. This JSX source scan does not prove the CSS/rendered interface has no legibility defects.

## Browser overlay evidence

Mutable preflight succeeded: title appended `[Human]` and script-tag execution set `window.__tuckMutablePreflight=true`. `/detect.js` was successfully injected and ran in empty, settings, and transcript states. Initial console counts were **8 / 5 / 10** element groups, respectively. Confirmation console counts were **44 transcript / 4 settings**. Counts differ because of ongoing UI edits, viewport/content differences, and the browser detector including offscreen text. These are grouped element counts, not unique-rule counts.

**No reliable user-visible overlay was presented.** Execution was in headless Chrome; `[Human]` is the successful preflight title, not a claim that a visible human browser tab exists. Browser inspection fallback and raw evidence are retained.

### Transcript confirmation

Serialized readback: 50 element groups; rules `cramped-padding` 1, `line-length` 10, `text-occlusion` 6, `tiny-text` 7, `undersized-ui-text` 27. Browser readback happened after overlay rendering; exclude detector-label `text-occlusion` rows as false positives.

| Rule | Exact browser selector | Evidence |
|---|---|---|
| `undersized-ui-text` | `span.preview-badge` | 10px functional text "PREVIEW" (below 11px floor) |
| `tiny-text` | `small` | 11px body text |
| `tiny-text` | `div.example-note > span` | 11px body text |
| `cramped-padding` | `div.document-tabs-row` | <div> "document-tabs-row": children flush against border-bottom on bottom (no inset) |
| `undersized-ui-text` | `span.tab-count` | 10px functional text "10" (below 11px floor) |
| `undersized-ui-text` | `#segment-0c79014e-ba2a-4184-bb96-b128dacc4d8d > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "You" (below 11px floor) |
| `undersized-ui-text` | `#segment-0c79014e-ba2a-4184-bb96-b128dacc4d8d > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "00:00" (below 11px floor) |
| `line-length` | `#segment-0c79014e-ba2a-4184-bb96-b128dacc4d8d > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-51413dbc-7f4b-463c-b56f-28d99ca00ab7 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-51413dbc-7f4b-463c-b56f-28d99ca00ab7 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "00:16" (below 11px floor) |
| `line-length` | `#segment-51413dbc-7f4b-463c-b56f-28d99ca00ab7 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-9458f294-1fc7-4d88-abc4-1362efda4be5 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "You" (below 11px floor) |
| `undersized-ui-text` | `#segment-9458f294-1fc7-4d88-abc4-1362efda4be5 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "00:39" (below 11px floor) |
| `line-length` | `#segment-9458f294-1fc7-4d88-abc4-1362efda4be5 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-4055f5da-b80e-4a85-9f9c-74fb0d497e8d > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-4055f5da-b80e-4a85-9f9c-74fb0d497e8d > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "00:55" (below 11px floor) |
| `line-length` | `#segment-4055f5da-b80e-4a85-9f9c-74fb0d497e8d > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-5134ab13-239d-4130-80b2-75cb67b43c95 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-5134ab13-239d-4130-80b2-75cb67b43c95 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "01:19" (below 11px floor) |
| `line-length` | `#segment-5134ab13-239d-4130-80b2-75cb67b43c95 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-d10796f5-de2f-4a68-b033-548156cbe606 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "You" (below 11px floor) |
| `undersized-ui-text` | `#segment-d10796f5-de2f-4a68-b033-548156cbe606 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "01:45" (below 11px floor) |
| `line-length` | `#segment-d10796f5-de2f-4a68-b033-548156cbe606 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-f40e7022-1ddf-4eae-9ae7-bc7f274a985f > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-f40e7022-1ddf-4eae-9ae7-bc7f274a985f > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "02:05" (below 11px floor) |
| `line-length` | `#segment-f40e7022-1ddf-4eae-9ae7-bc7f274a985f > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-8c5294b4-7283-4532-b2cd-02024390b665 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-8c5294b4-7283-4532-b2cd-02024390b665 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "02:27" (below 11px floor) |
| `line-length` | `#segment-8c5294b4-7283-4532-b2cd-02024390b665 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-411ae177-dde5-458d-95ed-04fd2f870c2b > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "You" (below 11px floor) |
| `undersized-ui-text` | `#segment-411ae177-dde5-458d-95ed-04fd2f870c2b > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "02:51" (below 11px floor) |
| `line-length` | `#segment-411ae177-dde5-458d-95ed-04fd2f870c2b > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `#segment-17ac6f3a-f585-407a-8706-4568db294701 > div.segment-body > div.segment-heading > span.segment-source` | 10px functional text "Computer audio" (below 11px floor) |
| `undersized-ui-text` | `#segment-17ac6f3a-f585-407a-8706-4568db294701 > div.segment-body > div.segment-heading > button.segment-time` | 10px functional text "03:15" (below 11px floor) |
| `line-length` | `#segment-17ac6f3a-f585-407a-8706-4568db294701 > div.segment-body > p` | ~94 chars/line (aim for <80) |
| `undersized-ui-text` | `div.transcript-end > span:nth-of-type(2)` | 10px functional text "End of transcript" (below 11px floor) |
| `tiny-text` | `div.document-status > span:nth-of-type(1)` | 10px body text |
| `tiny-text` | `div.document-status > span:nth-of-type(2)` | 10px body text |
| `undersized-ui-text` | `div.document-status > span:nth-of-type(2)` | 10px functional text "Local intelligence" (below 11px floor) |
| `undersized-ui-text` | `span:nth-of-type(2) > button` | 10px functional text "1 moment" (below 11px floor) |
| `tiny-text` | `div.assistant-context` | 11px body text |
| `undersized-ui-text` | `span.context-count` | 10px functional text "10" (below 11px floor) |
| `tiny-text` | `p.assistant-example-hint` | 11px body text |
| `undersized-ui-text` | `button.composer-context` | 10px functional text "Local model" (below 11px floor) |
| `tiny-text` | `p.assistant-footnote` | 10px body text |
| `text-occlusion` | `div:nth-of-type(3) > div > span` | span "tiny body text" is 100% covered by overlapping text (strong) |
| `text-occlusion` | `div:nth-of-type(8) > div > span` | span "undersized functional te" is 36% covered by an opaque element (aside.assistant-pane) |
| `text-occlusion` | `div:nth-of-type(11) > div > span` | span "undersized functional te" is 36% covered by an opaque element (aside.assistant-pane) |
| `text-occlusion` | `div:nth-of-type(12) > div > span` | span "line length too long" is 90% covered by overlapping text (button.speaker-name) |
| `text-occlusion` | `div:nth-of-type(42) > div > span` | span "undersized functional te" is 100% covered by overlapping text (div.assistant-context) |
| `text-occlusion` | `div:nth-of-type(45) > div > span` | span "tiny body text" is 88% covered by an opaque element (form.assistant-composer) |

### Settings confirmation

Serialized readback: 6 element groups; rules `low-contrast` 1, `text-occlusion` 2, `tiny-text` 2, `undersized-ui-text` 1. Browser readback happened after overlay rendering; exclude detector-label `text-occlusion` rows as false positives.

| Rule | Exact browser selector | Evidence |
|---|---|---|
| `undersized-ui-text` | `span.preview-badge` | 10px functional text "PREVIEW" (below 11px floor) |
| `low-contrast` | `button.sidebar-link.selected > kbd` | 4.3:1 (need 4.5:1) — text #616d83 on #e5e9f3 |
| `tiny-text` | `div > small` | 11px body text |
| `tiny-text` | `div.model-choice-summary > span` | 11px body text |
| `text-occlusion` | `div:nth-of-type(4) > div > span` | span "tiny body text" is 100% covered by overlapping text (strong) |
| `text-occlusion` | `div:nth-of-type(5) > div > span` | span "tiny body text" is 100% covered by an opaque element (div.model-choice-summary) |

## Confirmed actionable evidence

1. **P2 — weak assistant keyboard focus.** `src/styles.css:3`, `.assistant-composer textarea { outline:none }` overrides the generic `textarea:focus-visible` rule at line 2. Focused textarea computed `outline: rgb(41,46,58) none 3px`, with `:focus-visible=true`; the parent only changes to `1px solid #a9b8e8`. Keep the parent cue, add a strong textarea or composer focus-visible outline. Source consumer: `src/App.tsx:104` assistant composer.

2. **P2 — two remaining contrast misses in Settings.** Clean confirmation (before any injected overlays) calculates selected sidebar shortcut `#616d83` on `#e5e9f3` at **4.30:1** (`button.sidebar-link.selected > kbd`, `src/App.tsx:61` sidebar settings control; CSS token/rule `src/styles.css:1–3`). `div.model-choice-summary > span` (“Load models to check what is available.”) is `#657793` on `#f7f9fc`, **4.32:1**, 11px, also under 4.5:1. Set text to a darker color. At this readback, clean empty/recap/transcript measured **31/72/100** text elements with zero calculated failures; Settings measured **49**, two failures. This is computed-color sampling for text nodes, not a full WCAG conformance audit, and excludes disabled controls.

3. **P2 — functional microtext remains.** Browser flags exact `.segment-time`, `.segment-source`, `.composer-context`, `.context-count`, `.tab-count`, `.preview-badge` at 10px. `src/styles.css:3` contains these declarations; consumers `src/App.tsx:54`, `src/App.tsx:79`, `src/App.tsx:90`, `src/App.tsx:97`, and `src/App.tsx:104`. Transcript timestamps are useful navigation actions, so give them a readable size and a larger hit area. Tiny 10/11px labels matter more than decorative brand marks. Full selector locations and measurements are above. The line-length warning is an estimated 94 chars/line on transcript paragraphs; review as a reading comfort preference, not a functional blocker.

## False positives / excluded evidence

- Browser `cramped-padding` on `.document-tabs-row` is a conventional tab underline; the boundary intentionally joins the tab indicators, so additional container padding is not warranted.
- Browser serialized `text-occlusion` entries whose text is “tiny body text”, “undersized functional text”, or “line length too long” are the detector’s own label chrome, not product text. They arose in a readback after rendering overlays.
- Initial generic contrast scan included overlay OKLCH colors, which its lightweight RGB parser does not handle. Those bogus ~1.04:1 rows are excluded. The confirmation runs all contrast measurement before injection.
- Initial desktop document scrollWidth 1442 vs viewport 1440 occurred only after injection. App container widths were exact; clean confirmation is 1440/1440. Do not file that as app overflow.
- A single generic resource 404 console error appeared; no JS page errors. The failed resource was not identified, so no claim of an app route/network defect is made.

## Behavior and responsive evidence

| Flow | Observed outcome |
|---|---|
| Initial empty library | Welcome offers Start a meeting/Open an example and states browser recording/AI limits. |
| Example | Loaded only when requested; visibly labeled illustrative. |
| Notes autosave | “Saved locally” after editing; synthetic notes remained when switching meetings. |
| Recap citation | `01:19` opened Transcript and highlighted the intended existing-Notion-database segment; Markdown timestamp button implementation is `src/components/Markdown.tsx:10`. |
| Transcript no results | “No matching words”, shorter-phrase suggestion, Clear search; clear restored 10 example segments. |
| Preview AI error | Specific desktop/settings recovery; typed prompt preserved after failure. |
| Preview model setup | Explicit desktop-only recovery error, no audio captured. |
| New meeting | Creates untitled draft, selects Notes, explains model setup. |
| Library search/favorites | Clear search action for unmatched query; useful empty Starred state. |
| Conversation tab keyboard | Notes + ArrowRight moved focus and selected Transcript in the current HMR version. |
| Mobile navigation | Library search and assistant heading reachable through labeled bottom navigation. |

Clean viewport measurements: **390, 760, 1440 px** recap: document width exactly viewport; no app container horizontal overflow. At 1440 empty/transcript/settings also clean. Initial 390 checks covered notes, library, assistant, and each Settings tab with no horizontal overflow. Desktop side panels adapt to two/one pane appropriately. This is browser preview verification; native OS capture and mobile capture capabilities are not inferred.

## Artifact and cleanup provenance

Existing supplied screenshots visually inspected: `.impeccable/review/desktop.png`, `settings.png`, `compact.png`; existing `empty.png` and `transcript.png` were present. Independent pass captures: `.impeccable/review/b-{empty,desktop,settings,transcript,compact}.png`. These are evidence captured during HMR, not final release screenshots. Full structured evidence: `.impeccable/review/b-evidence.json` and `.impeccable/review/b-confirmation.json`.

Vite dev server at 1420 was pre-existing/shared; B did not start or stop it. B started only the Impeccable live-server helper (pid 73488, port 8400), stop method `node /Users/sglyon/.agents/skills/impeccable/scripts/live-server.mjs stop`. Cleanup is recorded below. Temporary browser scripts and detector JSON under `/tmp` are removed after copying the retained JSON.

Questions skipped: Assessment B subtask; user questions are owned by the root critique/synthesis.

Cleanup completed: Impeccable helper stop command succeeded; temporary scripts and temporary detector JSON removed. Isolated headless browser contexts closed. Shared Vite server retained.

Stop-helper caveat: server termination succeeded, but it reported `config_missing` for `.impeccable/live/config.json` while trying to remove a live script tag. No repo live script tag was injected by B; only per-page `detect.js` in now-closed temporary browser contexts. There is therefore no retained page injection to remove. File line references reflect source as read during the pass and can shift under the concurrent UI edits; exact CSS/DOM selectors are the stable locations.
