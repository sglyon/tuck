# Library loading UI — independent Impeccable review

**Disposition: SHIP for the reviewed interface design.** Final scores are **32/40 selected-detail loading/retry**, **31/40 library pagination/search/facets**, and **32/40 global task inbox**. Every changed area meets the user's 30/40 requirement. The compact Retry reachability issue is fixed and confirmed in actual pixels. No further design iteration is required.

## Method and evidence

Assessment A was performed independently by `/root/workflow_core` using the Impeccable critique/audit playbooks, `DESIGN.md`, the Operate surface and `library-loading-contract.md`. I reviewed implementation and actual raster pixels before consulting the detector's aggregate findings. The reviewer did not edit UI source or run another detector.

All 22 authored captures were opened: `.impeccable/review/loading-{opening,headers,load-more,changed,detail-retry,facets,off-page,tasks,task-more,search,list-retry}-{desktop,mobile}.png`. Desktop is 1320×900; compact is 390×844. They show a scripted synthetic native contract, explicitly labeled as an example fixture, with 135 meeting headers, an on-demand conversation, 75 tasks and an omitted-facet scenario. Source/config are `capture-loading.spec.ts` and `loading.config.ts`; request fixtures are in `tests/ui/support/large-library.ts`.

These are actual rendered interface pixels, not a live native-library, device, account or performance test. The session's CUA inventory reported no browser surfaces and a locked Mac; I make no independent live-browser, accessibility-tree or hardware claim. Root's final frozen-source verification reports a successful production build, **seven unit tests and all 30 end-to-end tests**. Relevant test source was inspected, including six loading regressions and restore-filter retention. Native composition verification and the 1,000-meeting measurements are separate evidence in `library-loading-verification.md`; those measurements exclude WebKit, models and a real video call.

## Scores

All ten Nielsen heuristics apply; each area is scored out of 40.

| Heuristic | Selected detail | Library pages / scope | Global tasks |
|---|---:|---:|---:|
| Visibility of system status | 3 | 3 | 3 |
| Match with the real world | 4 | 3 | 4 |
| User control and freedom | 3 | 3 | 3 |
| Consistency and standards | 3 | 3 | 3 |
| Error prevention | 4 | 4 | 4 |
| Recognition rather than recall | 3 | 3 | 3 |
| Flexibility and efficiency | 3 | 3 | 3 |
| Aesthetic and minimalist design | 3 | 3 | 3 |
| Error recovery | 3 | 3 | 3 |
| Help and documentation | 3 | 3 | 3 |
| **Total** | **32/40** | **31/40** | **32/40** |

The new states preserve the cool porcelain desk, flat seams, ink prose and cornflower actions. Opening one conversation has a restrained progress indicator. A failed detail request has a clear heading and Try again; another meeting remains selectable. The assistant stays unavailable until the conversation exists, avoiding an invented empty detail state. Old replies are associated with their owner, and the chosen meeting remains independent of a filtered or refreshed sidebar.

Library navigation retains whole-library counts, explicit Load more conversations, global search and a calm changed-library notice. The notice says the current meeting stays in place, which matches the separate detail cache. Facet omissions are disclosed, and exact-name text input permits selecting a folder beyond the first 200 suggestions. Search results still show an actual source-opening action rather than requiring the owner to be present on the first header page.

The task inbox uses the entire selected scope, not cached meeting bodies. Each row keeps its meeting link nearby, and Load more tasks displays 50 of 75. Completion controls remain native checkboxes; pending/loading states prevent using an obsolete task row while a new selection is loading. At 390px, task prose, source links and pagination wrap within the pane. The document and fixed bottom navigation remain distinct.

## Findings and author fixes

| Source | Finding | Confirmation status |
|---|---|---|
| Independent early source review | All conversations passed `starred:false`, selecting only unstarred records and invalidating a bootstrap cursor whose selection omitted the filter. | Fixed by omitting the property for All and using the same canonical installed key. The regression checks first-page continuation, 100 loaded rows, Starred and an off-page starred owner; native integration independently exercises real cursor binding. |
| Independent final raster review | The initial `loading-list-retry-mobile.png` with expanded facets showed neither the library error nor Try again. Fixed controls left the flexible results area too little usable height. | Fixed: compact sidebar controls scroll within their own region, leaving at least 140px for results. The replacement pixels show the error and Try again together without scrolling the results. The new 390px regression verifies the action is in the viewport, the results-height floor, retry behavior and clearing a failed query. |
| Parallel native reviewer / root | A restore could reinstall unfiltered headers without reapplying an existing selection; a delayed task write could refresh using an obsolete Show completed value; incoming speech could reset already-loaded pages. | Source shows an install epoch, effect-owned task refresh and background header/count updates. Regressions cover restore with a retained query, delayed completion while changing Show completed, and 100 loaded headers surviving the five-second background refresh. These findings are attributed to their reviewers. |

The redundant supporting line in the generic detail error was also removed. Some inherited compact actions are below 44px; a complete motor-accessibility or screen-reader qualification has not been performed.

## Bounded confirmation

I opened the five requested replacements: compact library Retry, facets and Load more, plus desktop/compact detail Retry. The compact fix preserves the navigation and footer while making both the library failure and its action visible. Expanded filter contents remain reachable through their own scroll region, and conversation pagination retains its explicit action. The author also corrected an attempted-selection key discovered during capture: clearing a failed query now reloads the original 50 headers. The final focused loading suite covers that transition.

One refreshed desktop error image contained a duplicated app strip along its bottom. The author replaced that single frame with a settled capture at a fixed viewport; I opened the replacement and confirmed a clean full desktop layout and the concise error action. This was evidence replacement, not another UI design iteration. The final review has no unresolved material findings.

The cache protects selected/active meetings, pending note and recap writes, correction drafts, chat and operation owners while retaining at most five additional idle details. Draft text remains tied to meeting identity. Source and tests support those ownership guarantees; screenshots alone cannot establish them. Core tests separately verify old-library defaults, deterministic timestamp ties, bounded requests, global facets/tasks, corrected-text matching, transactional cursor invalidation and opening an unrelated meeting despite malformed unselected evidence.

## Detector and limits

The author's complete `loading-detector.json` contains **315 advisory findings**: 198 color, 109 font-size and eight radius findings, all in `src/styles.css`. There are no non-advisory or feature-component findings in that artifact. The count includes inherited literal-token drift and two new text sizes. This is not a clean-stylesheet or full WCAG claim. Labels, native controls, status/alert roles and visible focus styling were checked in source and the available pixels; complete keyboard order, screen-reader announcements, zoom and reduced-motion behavior were not independently exercised.

No personal library, account, calendar or audio was opened. No detector rerun, context rerun, overlay injection or reviewer UI edits were performed. One final raster assessment and one bounded confirmation are the review limit.
