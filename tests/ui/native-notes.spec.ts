import { expect, test, type Page } from "@playwright/test";
import { exampleMeeting } from "../../src/lib/example";
import { defaults } from "../../src/lib/bridge";

async function nativeNotesPort(page: Page) {
  const meeting = exampleMeeting();
  meeting.notes = "";
  await page.addInitScript(
    ({ meeting, settings }) => {
      const state = window as unknown as Record<string, any>;
      const callbacks = new Map<number, (event: unknown) => Promise<void>>();
      let callbackId = 0;
      let closeHandler = 0;
      let saved = structuredClone(meeting);
      state.__savedNotes = "";
      state.__savedRecap = saved.summary;
      state.__closedRecap = null;
      state.__noteWrites = [];
      state.__closedNotes = null;
      state.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      state.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback(callback: (event: unknown) => Promise<void>) {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        async invoke(command: string, args: Record<string, any>) {
          if (command === "plugin:event|listen") {
            if (args.event === "tauri://close-requested")
              closeHandler = args.handler;
            return args.handler;
          }
          if (command === "plugin:event|unlisten") return;
          if (command === "plugin:window|close") {
            return callbacks.get(closeHandler)?.({
              event: "tauri://close-requested",
              id: closeHandler,
            });
          }
          if (command === "plugin:window|destroy") {
            state.__closedNotes = saved.notes;
            state.__closedRecap = saved.summary;
            return;
          }
          if (command !== "engine_call") return;
          if (args.method === "bootstrap")
            return {
              meetings: [saved],
              settings,
              accounts: [],
              status: {
                engineVersion: "test-port",
                platform: "test",
                modelInstalled: true,
                modelDownloading: false,
                modelProgress: 0,
                speakerModelInstalled: false,
                activeMeetingId: null,
                recordingState: "idle",
                captureAvailable: false,
              },
            };
          if (args.method === "hide_window") {
            state.__closedNotes = saved.notes;
            state.__closedRecap = saved.summary;
            return;
          }
          if (args.method === "update_meeting") {
            const notes =
              args.params.changes.notes ?? args.params.changes.summary;
            state.__noteWrites.push(`start:${notes}`);
            // Let an earlier write finish after a later debounce would normally run.
            await new Promise((resolve) =>
              setTimeout(
                resolve,
                notes === "Older notes" || notes === "Older recap" ? 1000 : 50,
              ),
            );
            saved = { ...saved, ...args.params.changes };
            state.__savedNotes = saved.notes;
            state.__savedRecap = saved.summary;
            state.__noteWrites.push(`saved:${notes}`);
            return saved;
          }
        },
      };
      state.__requestClose = () =>
        callbacks.get(closeHandler)?.({
          event: "tauri://close-requested",
          id: closeHandler,
        });
    },
    { meeting, settings: defaults },
  );
  await page.goto("/");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
}

test("delayed native note writes cannot overwrite newer notes", async ({
  page,
}) => {
  await nativeNotesPort(page);
  const notes = page.getByRole("textbox", {
    name: "Meeting notes",
    exact: true,
  });
  await notes.fill("Older notes");
  await expect
    .poll(() => page.evaluate(() => (window as any).__noteWrites))
    .toContain("start:Older notes");
  await notes.fill("Newest notes");
  await expect
    .poll(() => page.evaluate(() => (window as any).__savedNotes))
    .toBe("Newest notes");
  expect(await page.evaluate(() => (window as any).__noteWrites)).toEqual([
    "start:Older notes",
    "saved:Older notes",
    "start:Newest notes",
    "saved:Newest notes",
  ]);
});

test("native close request flushes notes before hiding the window", async ({
  page,
}) => {
  await nativeNotesPort(page);
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Keep this last thought.");
  await page.evaluate(() => (window as any).__requestClose());
  await expect
    .poll(() => page.evaluate(() => (window as any).__closedNotes))
    .toBe("Keep this last thought.");
});

test("recap writes stay ordered and closing flushes both editors", async ({
  page,
}) => {
  await nativeNotesPort(page);
  await page.getByRole("tab", { name: "Recap", exact: true }).click();
  await page.getByRole("button", { name: "Edit recap", exact: true }).click();
  const recap = page.getByRole("textbox", {
    name: "Edit meeting recap",
    exact: true,
  });
  await recap.fill("Older recap");
  await expect
    .poll(() => page.evaluate(() => (window as any).__noteWrites))
    .toContain("start:Older recap");
  await recap.fill("Newest recap");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Keep both editors.");
  await page.evaluate(() => (window as any).__requestClose());
  await expect
    .poll(() => page.evaluate(() => (window as any).__closedNotes))
    .toBe("Keep both editors.");
  expect(await page.evaluate(() => (window as any).__closedRecap)).toBe(
    "Newest recap",
  );
});
