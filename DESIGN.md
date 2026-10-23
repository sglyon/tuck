---
name: Tuck
description: A cool porcelain meeting desk with spoken evidence close at hand.
colors:
  accent: "#4d65cb"
  accent-dark: "#3d51af"
  accent-soft: "#e9edff"
  paper: "#fff"
  shell: "#f4f5f8"
  sidebar: "#f0f2f7"
  assistant: "#f8f9fc"
  ink: "#292e3a"
  muted: "#566278"
  faint: "#616d83"
  line: "#e7eaf0"
  green: "#348064"
  red: "#af4952"
  supporting-strong: "#536179"
typography:
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "26px"
    fontWeight: 650
    lineHeight: 1.28
    letterSpacing: "-0.8px"
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "14px"
    lineHeight: 1.85
  label:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "13px"
    fontWeight: 550
  supporting:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "12px"
rounded:
  citation: "3px"
  field: "5px"
  control: "6px"
  choice: "7px"
  surface: "8px"
  composer: "9px"
  dialog: "12px"
spacing:
  xs: "6px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "20px"
  section: "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.paper}"
    rounded: "{rounded.control}"
    padding: "9px 13px"
    typography: "{typography.label}"
  button-primary-hover:
    backgroundColor: "{colors.accent-dark}"
  button-secondary:
    backgroundColor: "{colors.paper}"
    textColor: "#596780"
    rounded: "{rounded.control}"
    padding: "9px 13px"
  button-subtle:
    backgroundColor: "#f2f4fb"
    textColor: "#5464a4"
    rounded: "{rounded.control}"
    padding: "9px 13px"
  button-danger:
    backgroundColor: "#b34f5c"
    textColor: "{colors.paper}"
    rounded: "{rounded.control}"
    padding: "9px 13px"
  input:
    backgroundColor: "{colors.paper}"
    textColor: "#5e6c88"
    rounded: "{rounded.field}"
    padding: "9px 11px"
  library-navigation:
    textColor: "#606b80"
    rounded: "{rounded.field}"
    padding: "10px 9px"
  provider-choice:
    textColor: "#5f6878"
    rounded: "{rounded.choice}"
    padding: "14px 11px"
  citation:
    backgroundColor: "#edf1fb"
    textColor: "#53669d"
    rounded: "{rounded.citation}"
    padding: "0 4px"
---

# Design System: Tuck

## Overview

**Creative North Star: "The Cool Porcelain Meeting Desk"**

Tuck is a meeting desk: a library, a document, and an assistant beside the conversation. Cool porcelain surfaces, flat seams, ink text, and cornflower actions give busy professionals a quiet place to capture a call and recover its evidence. System typography and familiar controls make the work readable before exposing model or connection details.

This is a code-first system documented from the final React/CSS implementation and the desktop, compact, settings, empty, and transcript captures in `.impeccable/review/`. Product purpose remains in `PRODUCT.md`; the selected surface strategy remains in `.impeccable/surfaces/meeting-workspace.md`.

**Key Characteristics:**
- Flat split panes with independent scrolling.
- Reading-first document hierarchy and restrained functional color.
- Timestamp citations return to spoken evidence.
- Plain-language setup with engineering details disclosed progressively.

## Colors

The palette combines white reading paper with cool porcelain surroundings and a concentrated cornflower action color. Frontmatter values reflect the final CSS cascade, including darkened supporting colors.

### Primary

- **Cornflower action** (`accent`): primary buttons, focus rings, active tabs, and links.
- **Deep cornflower** (`accent-dark`): primary-button hover.
- **Cornflower wash** (`accent-soft`): related selected and contextual surfaces; individual controls retain their implemented local shades.

### Neutral

- **Paper** (`paper`): the document and input surfaces.
- **Porcelain shell** (`shell`), **library porcelain** (`sidebar`), and **assistant porcelain** (`assistant`): distinguish the work areas without floating panels.
- **Ink** (`ink`): primary text. **Blue-gray support** (`muted`, `faint`): the final shared secondary hierarchy.
- **Strong supporting blue gray** (`supporting-strong`): model-readiness copy and keyboard shortcuts.
- **Cool seam** (`line`): one-pixel section and pane dividers.

Green identifies local/ready status; rose identifies recording, failure, or destructive context. Status always also has a label or recognizable control.

**The Functional Accent Rule.** Reserve cornflower for actions, selection, focus, and source navigation; use text and icons alongside status colors.

## Typography

Use the native system stack in frontmatter. There is no decorative display family. Monospace appears only in code or literal data; compact counts, elapsed time, transcript timestamps, and citations use tabular numerals where implemented.

The editable meeting title uses the title role. Recap and transcript prose use the body role and a maximum measure of 75ch. Main controls and navigation generally use 13px; essential supporting setup text generally uses 12–13px. The compact meeting title is 25px. The welcoming first-use headline is intentionally larger (37px desktop, 38px compact).

**The Readable Supporting Text Rule.** Preserve the final cascade: functional timestamp/source labels are 12px, context/tab counts are 11px, and model-readiness/shortcut text is 12px in the final dark blue gray. Small supplementary metadata remains a review limitation, not a new default.

The final two targeted Settings contrast corrections use `supporting-strong`: the independent review calculated 5.94:1 against the readiness surface and 5.15:1 against the shortcut background. These are targeted results, not a full WCAG audit. Inline recap citation chips retain their implemented smaller type; do not generalize that size to primary controls.

## Layout

The standard desktop shell is a 232px library, flexible document with a 400px minimum, and 340px assistant at full viewport height. Library and document content scroll independently; the assistant keeps its composer below its scrollable conversation. Header bars are 57px. Hiding the assistant or entering Settings removes its column.

At widths of at least 1550px the side columns grow to 250px and 375px. At 1190px and below they narrow to 212px and 300px with a 350px document minimum. Between 701px and 980px the library is 200px and the assistant replaces the document when selected. At 700px and below, one pane fills the screen, with a 63px bottom navigation for Library, Conversation, and Ask Tuck. The body supports a 320px minimum. Responsive UI does not establish native mobile audio capture.

The desktop document starts with an editable meeting name, date and duration, then Recap, Notes, Tasks, and Transcript. Capture controls appear for a draft or active meeting. Settings uses five sections—Intelligence, Audio & models, Writing, Notion, Privacy—and three provider choices; engineering controls live inside disclosure elements. Spacing follows compact repeated steps rather than a marketing-page rhythm. Keep toolbar and footer width while constraining long reading prose.

## Elevation & Depth

Persistent work areas are flat and distinguished by tone and seams. Menus use a downward shadow (`0 8px 30px #25385d26`), notifications a smaller shadow (`0 5px 22px #23365424`), and dialogs a broad shadow (`0 12px 55px #1a2a4f26`). Modal backdrops use a translucent ink wash and 2px blur. Exact elevation values also live in the sidecar.

**The Seams Before Shadows Rule.** Separate persistent panes through background tones and one-pixel seams; reserve shadows for floating menus, notifications, and dialogs.

## Shapes

Controls have small, gently rounded corners: the standard action uses the control radius, fields use the field radius, and provider choices use the choice radius. Surface groups and popovers use the surface radius; the assistant composer and dialogs have their own larger radii. The editable document title has square edges. Dots, avatars, and switch knobs use circular forms only where their role requires it. Do not impose one universal radius on every element.

## Components

- **Actions:** a cornflower primary button, white bordered secondary, pale subtle action, and rose destructive button. Standard buttons are at least 36px high with 9px × 13px padding; compact actions are at least 29px high with 7px × 10px padding. These are observed desktop dimensions, not blanket touch-target claims. Preserve hover, disabled, and keyboard-focus states.
- **Library and follow-through:** New meeting remains a persistent white bordered action; filters and selected conversations use quiet blue washes. Search remains light and integrated into the pane. Folder, tag and person filters are disclosed; the same scope applies to the conversation list, library answers and task inbox. Ask your library and Tasks open a single document-sized work area. Keyword matches show their meeting, source text and spoken position; AI answers show exact source links from the returned citation set. Changing scope starts a fresh library view so a late old-scope result cannot appear under a new filter. Per-meeting tasks retain a title, owner, due date and completion control; the inbox returns each commitment to its meeting. First launch is empty; Open an example is explicit and example content is labeled.
- **Document tabs and evidence:** four named views use a bottom rule and cornflower selection. Recap and assistant timestamps open the transcript and highlight the nearest moment. Navigation alone does not establish that generated claims are grounded correctly. Speaker correction uses a focused dialog; names are user-supplied.
- **Transcript corrections:** each inactive passage offers Correct words. The inline editor shows its unchanged source context, a disclosure for original speech recognition, and explicit Save correction / Cancel actions. Saved changes carry a Corrected by you label and a separate original-text disclosure with Restore original. The immutable ASR text remains distinct from effective corrected evidence; search, AI and exports use the corrected words with a marker. Existing recaps are not silently rewritten. Correction drafts are keyed by the pair of meeting and segment IDs, survive view changes and retain text on conflict. Hide, quit and file-operation guards return to an unsaved correction, clearing any transcript filter so Save/Cancel is reachable.
- **Retained audio and recovery:** a Saved audio disclosure in Transcript lists retained local WAV sources and their sizes. Imports accept WAV, MP3, FLAC and AAC-LC audio in M4A/MP4/AAC/ADTS; protected tracks and other codecs are not claimed. Compressed audio and WAV requiring conversion use a retained 16 kHz mono PCM WAV copy, with the selected original file untouched. Load audio resolves a scoped native URL and uses the browser's native audio controls with metadata-only preload. Playback never starts automatically; explicit timestamps can seek the currently loaded source without playing it. Original recordings are not loaded or decoded wholesale in JavaScript. Interrupted meetings offer Recover saved audio; recovery opens a separate linked meeting, with an Open original action and a clean recap/publication state. The source meeting's notes, original evidence, corrections and recording remain available. Empty/error states do not imply that audio was retained.
- **Portable library copies:** Privacy contains Save a backup and Restore from a backup as two ordinary sections. Audio inclusion is an explicit checkbox; backup reports show actual meeting/audio counts and warnings. Restore first previews new versus already-present meeting counts, archive audio counts and warnings, then requires Merge backup. The actual restore is bound to the previewed manifest fingerprint and revalidates the archive. Existing meetings are skipped as a unit, never overwritten or deleted. Results distinguish imported meeting text from any audio-copy warning. Both file operations share editor flushing, and restore refreshes the library only after its merge completes. Credentials are excluded; restored preferences do not activate hosted or automatic actions.
- **Notes and editable recaps:** both editors update local UI immediately and debounce persistence by 500ms. Writes are serialized per meeting and text target so a slower older save cannot overtake a later one. Only the currently pending version clears saving/error state. On failure, unsaved text remains available with a retry action. Recap generation, export, Notion publishing/updates, proposal generation and acceptance wait for relevant drafts. Browser unload warns about pending edits. Native close, tray-hide and quit requests flush both text editors and require an explicit save/cancel for dirty transcript corrections before hiding the persistent window or quitting. A save failure keeps the window open; these source guards and mocked tests do not prove the native close path on hardware.
- **Writing:** five built-in recap templates receive distinct instructions, with custom templates and saved recipes under Writing settings. Enhance notes/recap opens an inline instruction and recipe choice, with local or hosted-processing guidance beside Preview changes. Proposed text uses a pale cornflower surface and explicit Accept changes / Keep my text actions. Acceptance checks the original base text and retains the exact prior text in revision history. Manual edits are saved normally, while their history uses periodic checkpoints; the history caption explains this distinction without implying a version for each keystroke. Writing components are keyed by meeting and text target so delayed proposals cannot move into another document. No generated replacement is saved without acceptance.
- **Calendar and quick capture:** Upcoming is a quiet sidebar disclosure with device-calendar connection, explicit local ICS import, refresh and Prepare notes. Imports are labeled snapshots; recurring ICS rules are not expanded, and direct Google/Outlook sign-in is not presented as configured. Preparation never starts recording. Audio settings first offer a readiness check, then device selection and an explicit five-second sound test. Test audio is not saved or transcribed. Tray/global recording actions check only the selected sources, coalesce rapid intents and open setup if prerequisites are missing. Silence warnings use quiet transient information while recording continues; actual capture failures retain the persistent error treatment.
- **Assistant composer:** a white, lightly bordered surface with a 2px cornflower focus-within outline. The textarea uses its container focus treatment. Empty or unselected states disable prompts; an activity indicator requires a real meeting and stream. Questions are retained on preview failure. Hosted-processing guidance sits near the composer.
- **Settings:** present provider choice, readiness, and the setup action first. Connection URL, manual model ID, CPU/release controls, and engine/platform audio detail belong in Advanced or Technical details. Hosted processing consent remains off by default. Credentials travel to the native engine and are not stored in browser storage. Notion publishing is deliberate or an explicit automatic preference. Discovery lists shared destinations; optional metadata mapping uses compatible existing columns. Updates refresh Tuck-managed recap, personal-note and task sections while preserving other Notion content. An uncertain first creation stays check-first, with a separate existing-page linking action that verifies its meeting marker.
- **Recovery and keyboard behavior:** errors remain visible until dismissed; successful notices are transient. Delete uses a native HTML dialog with cancel initially focused. Tabs support arrows, Home, and End; Escape dismisses menus/dialogs. Cmd/Ctrl-K focuses search, Cmd/Ctrl-J opens the assistant, and Cmd/Ctrl-comma opens Settings. Title and notes focus outlines remain visible. Reduced-motion mode removes transitions, animated scrolling, and the spinner animation.

The independent final review in `docs/ui-review-a.md` is **SHIP for interface design**: workspace 31/40, onboarding/library 31/40, Settings 30/40. Its five core screenshots establish visual composition and states; browser tests and sampled contrast have their stated scopes. The workflow confirmation in `docs/workflow-ui-review.md` is also **SHIP for interface design**: writing/proposals/editable recap 32/40, library/search/chat/organization/tasks 31/40, calendar/capture/settings including Notion 30/40. Workflow screenshots include clearly labeled mocked contracts; no hardware or live-service behavior is established by those fixtures. The safety-wave review in `docs/safety-ui-review.md` is **SHIP for interface design**: corrections/originals 32/40, audio/recovery 31/40, backup/restore 32/40. Its paused-seek evidence uses a Range-capable browser fixture, separate from native custom-protocol and hardware validation. Native microphone/system audio, permissions, real inference and speaker quality, model downloads, hardware performance, OAuth, hosted AI, Notion, and secure-store behavior remain outside the visual verdict.

## Do's and Don'ts

### Do:
- Do keep the library, document, and assistant hierarchy legible at every breakpoint.
- Do keep essential supporting text readable against its actual surface and preserve the final functional microtext overrides.
- Do show saving, failure, retry, capture, and processing states truthfully.
- Do disclose hosted transcript processing beside recap, proposal and assistant actions.
- Do restrict source navigation to actual retrieved citations and render Markdown images as plain text rather than loading remote content.
- Do retain pending notes and recaps, ordered writes, and hide/quit flushing when changing the editors.

### Don't:
- Don't treat example content, a configured provider label, or a navigable timestamp as proof of live inference or factual grounding.
- Don't expose raw model IDs, CPU controls, connection URLs, or audio backend names as basic setup choices.
- Don't replace flat pane seams with a dashboard of floating cards or decorative shadows.
- Don't infer native capture, hardware performance, or hosted-service reliability from browser screenshots.
