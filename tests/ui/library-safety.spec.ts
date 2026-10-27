import { installLeanLibraryPort } from "./support/lean-library";
import { test, expect, type Page } from "@playwright/test";
import { exampleMeeting } from "../../src/lib/example";
import { defaults } from "../../src/lib/bridge";

async function safetyPort(page: Page, duplicateSegments = false, unidentified = false) {
  await installLeanLibraryPort(page);
  const meeting = exampleMeeting();
  meeting.isExample = false;
  meeting.status = "interrupted";
  if (unidentified) meeting.segments[1].speakerId = "unassigned";
  await page.addInitScript(
    ({ meeting, settings, duplicateSegments }) => {
      const state = window as any;
      let original = structuredClone(meeting);
      const secondId = crypto.randomUUID();
      let meetings = [
        original,
        ...(duplicateSegments
          ? [
              {
                ...structuredClone(original),
                id: secondId,
                title: "Second saved meeting",
                segments: original.segments.map((s) => ({
                  ...s,
                  meetingId: secondId,
                })),
              },
            ]
          : []),
      ];
      let callbackId = 0;
      const engineListeners = new Set<number>();
      const callbacks = new Map();
      state.__fileCalls = [];
      state.__hidden = 0;
      state.__backup = null;
      state.__previews = 0;
      state.__restores = 0;
      state.__audioSources = [];
      state.__forceConflict = false;
      state.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      state.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback(callback: unknown) {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        async invoke(command: string, args: any) {
          if (command === "plugin:event|listen") {
            if (args.event === "engine-event") engineListeners.add(args.handler);
            return args.handler;
          }
          if (command === "plugin:dialog|save") {
            state.__fileCalls.push("save-picker");
            return "/tmp/tuck-test-backup.zip";
          }
          if (command === "plugin:dialog|open") {
            state.__fileCalls.push("open-picker");
            return "/tmp/tuck-test-backup.zip";
          }
          if (command === "plugin:event|unlisten") {
            if (args.event === "engine-event") engineListeners.delete(args.eventId);
            return;
          }
          if (command !== "engine_call") return;
          const p = args.params;
          const current = () =>
            meetings.find((m) => m.id === (p.meetingId || p.id))!;
          switch (args.method) {
            case "bootstrap":
              return {
                meetings,
                settings,
                accounts: [],
                status: { modelInstalled: true, captureAvailable: false },
              };
            case "get_live_coach":
              if (!current()) throw "Choose an available fixture meeting.";
              return { enabled: false, status: ["complete", "interrupted"].includes(current().status) ? "stopped" : "off", message: null, selectedAgentIds: [], suggestions: [] };
            case "update_meeting":
              state.__fileCalls.push("save-draft");
              await new Promise((r) => setTimeout(r, 90));
              Object.assign(current(), p.changes);
              return current();
            case "meeting_audio":
              return [
                {
                  id: "microphone.wav",
                  name: "Example microphone",
                  bytes: 480044,
                  duration: 30,
                  repairable: true,
                },
              ];
            case "audio_source":
              state.__audioSources.push(p);
              return { url: location.origin + "/fixture-audio.wav" };
            case "recover_audio": {
              const source = current();
              const recovered = {
                ...structuredClone(source),
                id: "recovered-meeting",
                title: "Recovered · " + source.title,
                recoveredFrom: source.id,
                status: "complete",
                summary: "",
                notionUrl: null,
                notionStatus: "idle",
              };
              meetings.push(recovered);
              return recovered;
            }
            case "correct_transcript":
            case "reset_transcript": {
              const m = current();
              const s = m.segments.find((s) => s.id === p.segmentId)!;
              if (state.__forceConflict || s.text !== p.baseText)
                throw "This passage changed. Reopen it to use the latest text.";
              s.originalText ??= s.text;
              s.text =
                args.method === "reset_transcript" ? s.originalText : p.text;
              s.edited = s.text !== s.originalText;
              return m;
            }
            case "hide_window":
              state.__hidden++;
              return;
            case "backup_library":
              state.__fileCalls.push("backup");
              state.__backup = {
                notes: original.notes,
                summary: original.summary,
                includeAudio: p.includeAudio,
              };
              return {
                path: p.path,
                meetingCount: meetings.length,
                audioIncluded: p.includeAudio,
                audioFiles: p.includeAudio ? 1 : 0,
                warnings: [],
              };
            case "preview_restore_library":
              state.__fileCalls.push("preview");
              state.__previews++;
              return {
                meetingCount: 4,
                restorable: 2,
                skipped: 2,
                audioFiles: 3,
                warnings: [],
                settingsMode: "saved-writing-merged",
                fingerprint: "a".repeat(64),
              };
            case "restore_library":
              if (state.__forceRestoreConflict)
                throw "This backup changed after preview. Preview it again before restoring.";
              state.__fileCalls.push("restore");
              state.__restores++;
              state.__restoreFingerprint = p.expectedFingerprint;
              return {
                restored: 2,
                skipped: 2,
                audioFilesRestored: 2,
                settingsMode: "saved-writing-merged",
                warnings: [
                  "One audio file could not be restored; the meeting text was imported.",
                ],
              };
          }
        },
      };
      state.__intent = (intent: string) => {
        for (const id of engineListeners)
          callbacks.get(id)?.({
            event: "engine-event",
            id,
            payload: { event: "desktop_intent", payload: { intent } },
          });
      };
    },
    { meeting, settings: defaults, duplicateSegments },
  );
  await page.goto("/");
  return meeting;
}

test("corrections remain explicit, keep original speech recognition and can be reset", async ({
  page,
}) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  const segment = page.locator(".transcript-segment").first();
  const original = await segment.locator(".segment-body > p").textContent();
  await segment.hover();
  await segment
    .getByRole("button", { name: "Correct words", exact: true })
    .click();
  await segment
    .getByRole("textbox", { name: /Correction at/ })
    .fill("A corrected description of our handoff.");
  await segment
    .getByRole("button", { name: "Save correction", exact: true })
    .click();
  await expect(
    segment.getByText("Corrected by you", { exact: true }),
  ).toBeVisible();
  await expect(segment.locator(".segment-body > p")).toHaveText(
    "A corrected description of our handoff.",
  );
  await segment.getByText("Original transcript", { exact: true }).click();
  await expect(segment.locator(".original-transcript > p")).toHaveText(
    original!,
  );
  await page.reload();
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  await page
    .locator(".transcript-segment")
    .first()
    .getByText("Original transcript", { exact: true })
    .click();
  await page
    .getByRole("button", { name: "Restore original", exact: true })
    .click();
  await expect(
    page.locator(".transcript-segment").first().locator(".segment-body > p"),
  ).toHaveText(original!);
  await expect(page.getByText("Corrected by you", { exact: true })).toHaveCount(
    0,
  );
});

test("conflicted correction drafts survive view changes and block hide and file operations", async ({
  page,
}) => {
  await safetyPort(page);
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  const segment = page.locator(".transcript-segment").first();
  await segment.hover();
  await segment
    .getByRole("button", { name: "Correct words", exact: true })
    .click();
  await segment
    .getByRole("textbox", { name: /Correction at/ })
    .fill("Keep this correction draft.");
  await page.evaluate(() => ((window as any).__forceConflict = true));
  await segment
    .getByRole("button", { name: "Save correction", exact: true })
    .click();
  await expect(segment.getByRole("alert")).toContainText("passage changed");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toHaveValue("Keep this correction draft.");
  await page
    .getByRole("textbox", { name: "Search this transcript", exact: true })
    .fill("unmatched words hide this correction");
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toHaveCount(0);
  await page.evaluate(() => (window as any).__intent("hide"));
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toBeVisible();
  await expect(page.getByRole("alert").last()).toContainText("Save or cancel");
  expect(await page.evaluate(() => (window as any).__hidden)).toBe(0);
  await page.getByRole("button", { name: /^Settings/ }).click();
  await page.getByRole("tab", { name: "Privacy", exact: true }).click();
  await page
    .getByRole("button", { name: "Choose backup…", exact: true })
    .click();
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toHaveValue("Keep this correction draft.");
  expect(await page.evaluate(() => (window as any).__previews)).toBe(0);
  await segment.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.evaluate(() => (window as any).__intent("hide"));
  await expect
    .poll(() => page.evaluate(() => (window as any).__hidden))
    .toBe(1);
});

test("retained audio loads only on request, never autoplays, seeks on intent and recovers separately", async ({
  page,
}) => {
  const wav = Buffer.alloc(480044);
  wav.write("RIFF");
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(8000, 24);
  wav.writeUInt32LE(16000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(480000, 40);
  await page.route("**/fixture-audio.wav", (route) => {
    const range = route.request().headers().range;
    const match = range?.match(/bytes=(\d+)-(\d*)/);
    const start = match ? Number(match[1]) : 0;
    const end =
      match && match[2]
        ? Math.min(Number(match[2]), wav.length - 1)
        : wav.length - 1;
    return route.fulfill({
      status: match ? 206 : 200,
      contentType: "audio/wav",
      headers: {
        "accept-ranges": "bytes",
        ...(match
          ? { "content-range": `bytes ${start}-${end}/${wav.length}` }
          : {}),
      },
      body: wav.subarray(start, end + 1),
    });
  });
  const meeting = await safetyPort(page);
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  expect(await page.evaluate(() => (window as any).__audioSources)).toEqual([]);
  await page.locator(".retained-audio > summary").click();
  await page.getByRole("button", { name: "Load audio", exact: true }).click();
  const audio = page.locator("audio");
  await expect(audio).toHaveAttribute("preload", "metadata");
  await expect
    .poll(() => audio.evaluate((el: any) => el.readyState))
    .toBeGreaterThanOrEqual(1);
  expect(await audio.evaluate((el: any) => el.paused)).toBe(true);
  expect(await audio.getAttribute("autoplay")).toBeNull();
  await page.locator(".segment-time").nth(1).click();
  await expect
    .poll(() => audio.evaluate((el: any) => el.currentTime))
    .toBe(meeting.segments[1].start);
  expect(await audio.evaluate((el: any) => el.paused)).toBe(true);
  const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
  await timeline.getByRole("button", { name: "Explore Maya Chen's speech" }).focus();
  await page.keyboard.press("Home");
  await expect.poll(() => audio.evaluate((el: any) => el.currentTime)).toBe(meeting.segments[1].start);
  await expect(timeline.locator(".timeline-position")).toHaveText("00:16 / 03:35");
  expect(await audio.evaluate((el: any) => el.paused)).toBe(true);
  await audio.evaluate((el: HTMLAudioElement) => { el.currentTime = 8; });
  await expect(timeline.locator(".timeline-position")).toHaveText("00:08 / 03:35");
  await page
    .getByRole("button", { name: "Recover saved audio", exact: true })
    .last()
    .click();
  await expect(
    page.getByRole("textbox", { name: "Conversation title" }),
  ).toHaveValue("Recovered · " + meeting.title);
  await expect(
    page.getByText(
      "Recovered into a separate meeting. The original was kept.",
      { exact: true },
    ),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Open original", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Conversation title" }),
  ).toHaveValue(meeting.title);
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue(meeting.notes);
});

test("backup flushes both editors and restore previews counts before a fingerprint-bound merge", async ({
  page,
}) => {
  await safetyPort(page);
  await page.getByRole("button", { name: "Edit recap", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Edit meeting recap", exact: true })
    .fill("Latest recap for backup.");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Latest notes for backup.");
  await page.getByRole("button", { name: /^Settings/ }).click();
  await page.getByRole("tab", { name: "Privacy", exact: true }).click();
  await page
    .getByRole("checkbox", { name: "Include retained audio in backup" })
    .check();
  await page.getByRole("button", { name: "Save backup…", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__backup))
    .toEqual({
      notes: "Latest notes for backup.",
      summary: "Latest recap for backup.",
      includeAudio: true,
    });
  await page
    .getByRole("button", { name: "Choose backup…", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Backup preview" }),
  ).toContainText("New meetings to add");
  expect(await page.evaluate(() => (window as any).__restores)).toBe(0);
  await page.evaluate(() => ((window as any).__forceRestoreConflict = true));
  await page.getByRole("button", { name: "Merge backup", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "backup changed after preview",
  );
  expect(await page.evaluate(() => (window as any).__restores)).toBe(0);
  await page.evaluate(() => ((window as any).__forceRestoreConflict = false));
  await page
    .getByRole("button", { name: "Choose backup…", exact: true })
    .click();
  await page.getByRole("button", { name: "Merge backup", exact: true }).click();
  await expect(
    page.getByText("Restore complete", { exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).__restoreFingerprint)).toBe(
    "a".repeat(64),
  );
  await expect(
    page.getByText(
      "One audio file could not be restored; the meeting text was imported.",
      { exact: true },
    ),
  ).toBeVisible();
  await page.getByRole("navigation", {name:"Main navigation"}).getByRole("button",{name:/^Library/}).click();
  await page.locator(".meeting-item").first().click();
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Latest notes for backup.");
});

test("correction drafts belong to a meeting and segment together", async ({
  page,
}) => {
  const first = await safetyPort(page, true);
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  const segment = page.locator(".transcript-segment").first();
  await segment.hover();
  await segment
    .getByRole("button", { name: "Correct words", exact: true })
    .click();
  await segment
    .getByRole("textbox", { name: /Correction at/ })
    .fill("Draft for the first meeting only.");
  await page
    .locator(".meeting-item")
    .filter({ hasText: "Second saved meeting" })
    .click();
  await expect(page.locator(".correction-editor")).toHaveCount(0);
  await segment.hover();
  await segment
    .getByRole("button", { name: "Correct words", exact: true })
    .click();
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toHaveValue(first.segments[0].text);
  await segment
    .getByRole("textbox", { name: /Correction at/ })
    .fill("Correction for the second meeting only.");
  await segment
    .getByRole("button", { name: "Save correction", exact: true })
    .click();
  await page.locator(".meeting-item").filter({ hasText: first.title }).click();
  await expect(
    segment.getByRole("textbox", { name: /Correction at/ }),
  ).toHaveValue("Draft for the first meeting only.");
  await expect(segment.locator(".segment-body > p")).toHaveText(
    first.segments[0].text,
  );
});

test("restore reapplies an existing library query instead of displaying unfiltered headers", async ({
  page,
}) => {
  await safetyPort(page);
  await page
    .getByRole("textbox", { name: "Filter saved items", exact: true })
    .fill("No such saved conversation");
  await expect(page.locator(".meeting-item")).toHaveCount(0);
  await expect(
    page.getByText("No conversations found.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /^Settings/ }).click();
  await page.getByRole("tab", { name: "Privacy", exact: true }).click();
  await page
    .getByRole("button", { name: "Choose backup…", exact: true })
    .click();
  await page.getByRole("button", { name: "Merge backup", exact: true }).click();
  await expect(
    page.getByText("Restore complete", { exact: true }),
  ).toBeVisible();
  await page.getByRole("navigation", {name:"Main navigation"}).getByRole("button",{name:/^Library/}).click();

  await expect(
    page.getByRole("textbox", { name: "Filter saved items", exact: true }),
  ).toHaveValue("No such saved conversation");
  await expect(
    page.getByText("No conversations found.", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".meeting-item")).toHaveCount(0);
});


for (const width of [1320, 600]) {
  test(`transcript actions stay subordinate and unidentified text aligns at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await safetyPort(page, false, true);
    await page.getByRole("tab", { name: /^Transcript/ }).click();
    const rows = page.locator(".transcript-segment");
    const named = rows.first();
    const unknown = rows.nth(1);
    await named.scrollIntoViewIfNeeded();
    await expect(unknown.getByText("Unidentified", { exact: true })).toBeVisible();
    const textX = await named.locator(".segment-body > p").evaluate(el => el.getBoundingClientRect().left);
    const unknownX = await unknown.locator(".segment-body > p").evaluate(el => el.getBoundingClientRect().left);
    expect(Math.abs(textX - unknownX)).toBeLessThan(1);
    const pen = unknown.getByRole("button", { name: "Correct words", exact: true });
    await page.mouse.move(0,0);
    await expect(pen).toHaveCSS("opacity", "0");
    await unknown.hover();
    await expect(pen).toHaveCSS("opacity", "1");
    await page.screenshot({ path: testInfo.outputPath("transcript-hover.png") });
    const iconWidth = await pen.locator("svg").evaluate(el => el.getBoundingClientRect().width);
    const nameSize = await named.locator(".speaker-name").evaluate(el => parseFloat(getComputedStyle(el).fontSize));
    expect(iconWidth).toBeLessThan(nameSize);
    const penBox = await pen.boundingBox();
    const timeBox = await unknown.locator(".segment-time").boundingBox();
    expect(penBox!.x).toBeGreaterThan(timeBox!.x);
    await page.mouse.move(0,0);
    await page.screenshot({ path: testInfo.outputPath("transcript-rest.png") });
    await pen.focus();
    await expect(pen).toHaveCSS("opacity", "1");
    await pen.press("Enter");
    await expect(unknown.getByRole("textbox", { name: /Correction at/ })).toBeVisible();
    await unknown.getByRole("button", { name: "Cancel", exact: true }).click();
  });
}
