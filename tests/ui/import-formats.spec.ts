import { installLeanLibraryPort } from "./support/lean-library";
import { test, expect } from "@playwright/test";
import { exampleMeeting } from "../../src/lib/example";
import { defaults } from "../../src/lib/bridge";

test("native import offers supported formats, preserves the selected path and explains retained conversion", async ({
  page,
}) => {
  const meeting = exampleMeeting();
  meeting.isExample = false;
  await page.addInitScript(
    ({ meeting, settings }) => {
      const state = window as any;
      const meetings: typeof meeting[] = [];
      state.__importOptions = null;
      state.__importParams = null;
      state.__audioSourceCalls = 0;
      state.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      state.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback() {
          return 1;
        },
        async invoke(command: string, args: any) {
          if (command === "plugin:dialog|open") {
            state.__importOptions = args.options;
            return "/example/Original customer audio.m4a";
          }
          if (command !== "engine_call") return 1;
          if (args.method === "bootstrap")
            return {
              meetings,
              settings,
              accounts: [],
              status: { modelInstalled: true, captureAvailable: false },
            };
          if (args.method === "import_audio") {
            state.__importParams = args.params;
            meetings.push(meeting);
            return meeting;
          }
          if (args.method === "get_live_coach") {
            const selected = meetings.find((m) => m.id === args.params.meetingId);
            if (!selected) throw "Choose an available fixture meeting.";
            return { enabled: false, status: ["complete", "interrupted"].includes(selected.status) ? "stopped" : "off", message: null, selectedAgentIds: [], suggestions: [] };
          }
          if (args.method === "meeting_audio")
            return [
              {
                id: "import.wav",
                name: "Imported audio",
                bytes: 160044,
                duration: 5,
              },
            ];
          if (args.method === "audio_source") {
            state.__audioSourceCalls++;
            throw "Playback was not requested in this test.";
          }
        },
      };
    },
    { meeting, settings: defaults },
  );
  await page.goto("/");
  await page.locator(".new-menu > summary").click();
  await page.getByRole("button", { name: "Import audio…", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Conversation title" }),
  ).toHaveValue(meeting.title);
  const filter = await page.evaluate(
    () => (window as any).__importOptions.filters[0],
  );
  expect(filter.extensions).toEqual([
    "wav",
    "mp3",
    "flac",
    "m4a",
    "mp4",
    "aac",
    "adts",
  ]);
  expect(filter.extensions).not.toContain("opus");
  expect(filter.extensions).not.toContain("ogg");
  expect(await page.evaluate(() => (window as any).__importParams)).toEqual({
    path: "/example/Original customer audio.m4a",
  });
  await page.locator(".retained-audio > summary").click();
  await expect(
    page.getByText(
      "For imported compressed audio or WAV that needs conversion, the retained copy is 16 kHz mono PCM WAV. Your original file is unchanged.",
      { exact: true },
    ),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).__audioSourceCalls)).toBe(0);
});

// Shared fixture compatibility; production exclusively uses the lean protocol.
test.beforeEach(async ({ page }) => {
  await installLeanLibraryPort(page);
});
