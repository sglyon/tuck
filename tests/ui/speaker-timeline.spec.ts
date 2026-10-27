import { test, expect, type Page } from "@playwright/test";
import { exampleMeeting } from "../../src/lib/example";
import type { Meeting } from "../../src/lib/types";
import { installLargeLibrary } from "./support/large-library";

async function openTranscript(page: Page, meeting: Meeting = exampleMeeting()) {
  meeting.segments.forEach((segment, index) => { segment.id = `timeline-${index}`; });
  await page.addInitScript(meeting => {
    localStorage.setItem("tuck-preview-v1", JSON.stringify({ meetings: [meeting] }));
  }, meeting);
  await page.goto("/");
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  return meeting;
}

test("speaker lanes navigate exact passages, clear search, support keyboard turns and collapse", async ({ page }) => {
  const meeting = await openTranscript(page);
  const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
  await expect(timeline.getByRole("button", { name: /^Explore/ })).toHaveCount(3);
  const maya = timeline.getByRole("button", { name: "Explore Maya Chen's speech" });
  await page.getByRole("textbox", { name: "Search this transcript" }).fill("unmatched words");
  const bounds = (await maya.boundingBox())!;
  await maya.click({ position: { x: bounds.width * 60 / meeting.duration, y: 15 } });
  await expect(page.getByRole("textbox", { name: "Search this transcript" })).toHaveValue("");
  await expect(page.locator("#segment-timeline-3")).toHaveClass(/highlighted/);
  await expect(page.locator("#segment-timeline-3")).toBeInViewport();
  await expect(timeline.locator(".timeline-position")).toHaveText("00:55 / 03:35");
  await expect(maya).toBeFocused();
  await maya.press("ArrowRight");
  await expect(page.locator("#segment-timeline-6")).toHaveClass(/highlighted/);
  await maya.press("Home");
  await expect(page.locator("#segment-timeline-1")).toHaveClass(/highlighted/);
  await maya.press("End");
  await expect(page.locator("#segment-timeline-9")).toHaveClass(/highlighted/);
  const collapse = timeline.getByRole("button", { name: "Speaker timeline", exact: true });
  await collapse.click();
  await expect(collapse).toHaveAttribute("aria-expanded", "false");
  await expect(maya).toBeHidden();
  await collapse.click();
  await expect(maya).toBeVisible();
});

test("overlapping speakers navigate their own passage and labels follow renaming", async ({ page }) => {
  const meeting = exampleMeeting();
  meeting.segments[1].start = 0;
  await openTranscript(page, meeting);
  const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
  const maya = timeline.getByRole("button", { name: "Explore Maya Chen's speech" });
  await maya.focus();
  await maya.press("Home");
  await expect(page.locator("#segment-timeline-1")).toHaveClass(/highlighted/);
  await page.locator(".speaker-legend").getByRole("button", { name: "Maya Chen" }).click();
  await page.getByRole("textbox", { name: "Speaker name" }).fill("Maya Patel");
  await page.getByRole("button", { name: "Save name", exact: true }).click();
  await expect(timeline.getByRole("button", { name: "Explore Maya Patel's speech" })).toBeVisible();
});

test("unknown speakers stay explicit and the timeline belongs to the Transcript view", async ({ page }) => {
  const meeting = exampleMeeting();
  meeting.speakers = [];
  meeting.segments = [{ ...meeting.segments[0], speakerId: "" }];
  await openTranscript(page, meeting);
  const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
  await expect(timeline.getByRole("button", { name: "Explore Unassigned's speech" })).toBeVisible();
  await expect(timeline.locator(".timeline-share")).toHaveText("100%");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await expect(timeline).toHaveCount(0);
});

test("empty transcripts have no invented timeline", async ({ page }) => {
  const meeting = exampleMeeting();
  meeting.segments = [];
  await openTranscript(page, meeting);
  await expect(page.getByRole("region", { name: "Speaker timeline", exact: true })).toHaveCount(0);
});

test("incoming speech extends the timeline without losing its collapsed state", async ({ page }) => {
  await installLargeLibrary(page);
  await page.goto("/");
  await page.getByRole("tab", { name: /^Transcript/ }).click();
  const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
  const collapse = timeline.getByRole("button", { name: "Speaker timeline", exact: true });
  await collapse.click();
  await page.evaluate(() => {
    (window as any).__emit("segment", { meetingId: "meeting-000", segment: {
      id: "incoming-turn", meetingId: "meeting-000", start: 3601, end: 3610,
      speakerId: "speaker-9", source: "system", text: "A newly recognized speaker turn.",
    } });
  });
  await expect(timeline.locator(".timeline-position")).toContainText("1:00:10");
  await expect(collapse).toHaveAttribute("aria-expanded", "false");
  await collapse.click();
  const incoming = timeline.getByRole("button", { name: "Explore Speaker 9's speech" });
  await expect(incoming).toBeVisible();
  await incoming.focus();
  await incoming.press("Home");
  await expect(page.locator("#segment-incoming-turn")).toHaveClass(/highlighted/);
});

test("timeline stays visible while reading on desktop and compact screens", async ({ page }) => {
  await openTranscript(page);
  for (const width of [1320, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const timeline = page.getByRole("region", { name: "Speaker timeline", exact: true });
    await expect(timeline).toBeInViewport();
    await page.locator(".document-scroll").evaluate(element => { element.scrollTop = element.scrollHeight; });
    await expect(timeline).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
    if (process.env.TUCK_CAPTURE_TIMELINE) {
      await page.locator(".transcript-segment").first().scrollIntoViewIfNeeded();
      await page.screenshot({ path: `.impeccable/review/speaker-timeline-${width}.png` });
    }
  }
});
