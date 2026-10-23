import { installLeanLibraryPort } from "./support/lean-library";
import { test, expect } from "@playwright/test";
import { exampleMeeting } from "../../src/lib/example";
import { defaults } from "../../src/lib/bridge";
test("writing proposals flush drafts, preserve intervening edits and save only on acceptance", async ({
  page,
}) => {
  const meeting = exampleMeeting();
  meeting.notes = "Before";
  await page.addInitScript(
    ({ meeting, settings }) => {
      const state = window as unknown as Record<string, any>;
      let saved = meeting;
      const history: unknown[] = [];
      state.__writingCalls = [];
      state.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      state.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback() {
          return 1;
        },
        async invoke(command: string, args: Record<string, any>) {
          if (command !== "engine_call") return 1;
          const p = args.params;
          state.__writingCalls.push(args.method);
          if (args.method === "bootstrap")
            return {
              meetings: [saved],
              settings,
              accounts: [],
              status: { modelInstalled: true, captureAvailable: false },
            };
          if (args.method === "update_meeting") {
            await new Promise((r) => setTimeout(r, 80));
            saved = { ...saved, ...p.changes };
            return saved;
          }
          if (args.method === "propose_notes")
            return {
              baseText: saved.notes,
              text: "A reviewed next step.",
              target: "notes",
            };
          if (args.method === "apply_proposal") {
            if (saved.notes !== p.baseText)
              throw "Your notes changed. Generate a fresh proposal.";
            history.push({
              id: "revision-1",
              target: "notes",
              text: saved.notes,
              createdAt: new Date().toISOString(),
            });
            saved = { ...saved, notes: p.text };
            return saved;
          }
          if (args.method === "note_history") return history;
        },
      };
    },
    { meeting, settings: defaults },
  );
  await page.goto("/");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Newest draft");
  await page
    .getByRole("button", { name: "Enhance notes", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Writing instruction" })
    .fill("Pull out the decision.");
  await page
    .getByRole("button", { name: "Preview changes", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Proposed changes" }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Newest draft");
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("An intervening correction.");
  await page
    .getByRole("button", { name: "Accept changes", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("notes changed");
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("An intervening correction.");
  await page.getByRole("button", { name: "Keep my text", exact: true }).click();
  await page
    .getByRole("button", { name: "Preview changes", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Accept changes", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("A reviewed next step.");
  await page
    .getByRole("button", { name: "Revision history", exact: true })
    .click();
  await expect(
    page.getByText(
      "Manual versions are kept periodically. Accepted AI changes also keep the exact previous text.",
      { exact: true },
    ),
  ).toBeVisible();
  await page
    .getByRole("region", { name: "Revision history" })
    .locator("summary")
    .click();
  await expect(
    page.getByRole("region", { name: "Revision history" }),
  ).toContainText("An intervening correction.");
  const calls = await page.evaluate(() => (window as any).__writingCalls);
  expect(calls.indexOf("update_meeting")).toBeLessThan(
    calls.indexOf("propose_notes"),
  );
});

test("library answers reject forged autolink and reference destinations outside retrieved citations", async ({
  page,
}) => {
  const meeting = exampleMeeting();
  const imageRequests: string[] = [];
  await page.route("https://example.invalid/**", (route) => {
    imageRequests.push(route.request().url());
    void route.abort();
  });
  await page.addInitScript(
    ({ meeting, settings }) => {
      const good = `tuck://meeting/${meeting.id}?source=transcript&segment=${meeting.segments[0].id}&start=0`;
      (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener() {},
      };
      (window as any).__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback() {
          return 1;
        },
        async invoke(command: string, args: Record<string, any>) {
          if (command !== "engine_call") return 1;
          if (args.method === "bootstrap")
            return {
              meetings: [meeting],
              settings,
              accounts: [],
              status: { modelInstalled: true, captureAvailable: false },
            };
          if (args.method === "chat_library")
            return {
              id: "reply",
              role: "assistant",
              content: `[Verified source](${good}) <tuck://meeting/not-retrieved?source=summary> [Forged reference][x]\n\n![remote tracker](https://example.invalid/image?context=private)\n\n[x]: tuck://meeting/not-retrieved?source=summary`,
              createdAt: new Date().toISOString(),
              citations: [
                {
                  meetingId: meeting.id,
                  meetingTitle: meeting.title,
                  segmentId: meeting.segments[0].id,
                  start: 0,
                  source: "transcript",
                  text: meeting.segments[0].text,
                  url: good,
                },
              ],
            };
        },
      };
    },
    { meeting, settings: defaults },
  );
  await page.goto("/");
  await page
    .getByRole("button", { name: "Ask your library", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Search or ask your library" })
    .fill("What did we agree?");
  await page.getByRole("button", { name: "Ask Tuck", exact: true }).click();
  await expect(
    page.getByText("Image: remote tracker", { exact: true }),
  ).toBeVisible();
  expect(imageRequests).toEqual([]);
  await page
    .getByRole("button", {
      name: "tuck://meeting/not-retrieved?source=summary",
      exact: true,
    })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "not one of the retrieved sources",
  );
  await page
    .getByRole("button", { name: "Forged reference", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Ask your library", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Verified source", exact: true })
    .click();
  await expect(page.locator(".transcript-segment.highlighted")).toContainText(
    meeting.segments[0].text,
  );
});

test("a delayed proposal or search cannot move into a different meeting or folder scope", async ({
  page,
}) => {
  const first = exampleMeeting();
  first.title = "First example";
  first.notes = "";
  first.folder = "Alpha";
  const second = {
    ...exampleMeeting(),
    title: "Second example",
    notes: "",
    folder: "Beta",
  };
  await page.addInitScript(
    ({ first, second, settings }) => {
      const state = window as any;
      state.__proposalDone = false;
      state.__searchDone = false;
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
          if (command !== "engine_call") return 1;
          if (args.method === "bootstrap")
            return {
              meetings: [first, second],
              settings,
              accounts: [],
              status: { modelInstalled: true, captureAvailable: false },
            };
          if (args.method === "propose_notes") {
            await new Promise((r) => setTimeout(r, 700));
            state.__proposalDone = true;
            return {
              baseText: "",
              target: "notes",
              text: "Proposal for the first meeting only.",
            };
          }
          if (args.method === "search_library") {
            await new Promise((r) => setTimeout(r, 700));
            state.__searchDone = true;
            return [
              {
                meetingId: first.id,
                meetingTitle: first.title,
                text: "Old Alpha scope only.",
                source: "notes",
                url: `tuck://meeting/${first.id}?source=notes`,
              },
            ];
          }
        },
      };
    },
    { first, second, settings: defaults },
  );
  await page.goto("/");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("button", { name: "Enhance notes", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Writing instruction" })
    .fill("Summarize this meeting.");
  await page
    .getByRole("button", { name: "Preview changes", exact: true })
    .click();
  await page
    .locator(".meeting-item")
    .filter({ hasText: "Second example" })
    .click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__proposalDone))
    .toBe(true);
  await expect(
    page.getByRole("region", { name: "Proposed changes" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Ask your library", exact: true })
    .click();
  await page
    .getByText("Filter by folder, tag or person", { exact: false })
    .click();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .selectOption("Alpha");
  await page
    .getByRole("textbox", { name: "Search or ask your library" })
    .fill("scope");
  await page
    .getByRole("button", { name: "Search passages", exact: true })
    .click();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .selectOption("Beta");
  await expect
    .poll(() => page.evaluate(() => (window as any).__searchDone))
    .toBe(true);
  await expect(
    page.getByText("Old Alpha scope only.", { exact: true }),
  ).toHaveCount(0);
});

test("quick recording respects selected sources, coalesces rapid intents, and hides only after saving", async ({
  page,
}) => {
  const meeting = exampleMeeting();
  meeting.status = "draft";
  meeting.isExample = false;
  meeting.notes = "";
  await page.addInitScript(
    ({ meeting, settings }) => {
      const state = window as any;
      let saved = meeting;
      let callbackId = 0;
      const engineListeners = new Set<number>();
      const callbacks = new Map();
      state.__creates = 0;
      state.__starts = 0;
      state.__hiddenNotes = null;
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
          if (command === "plugin:event|unlisten") {
            if (args.event === "engine-event") engineListeners.delete(args.eventId);
            return;
          }
          if (command !== "engine_call") return;
          switch (args.method) {
            case "bootstrap":
              return {
                meetings: [saved],
                settings,
                accounts: [],
                status: { modelInstalled: true, captureAvailable: true },
              };
            case "get_live_coach":
              if (args.params.meetingId !== saved.id) throw "Choose an available fixture meeting.";
              return { enabled: false, status: ["complete", "interrupted"].includes(saved.status) ? "stopped" : "off", message: null, selectedAgentIds: [], suggestions: [] };
            case "capture_readiness":
              await new Promise((r) => setTimeout(r, 100));
              return {
                modelReady: true,
                helperReady: true,
                microphoneAvailable: true,
                systemAvailable: false,
              };
            case "create_meeting":
              state.__creates++;
              return { ...saved, id: "quick-draft" };
            case "start_recording":
              state.__starts++;
              saved = {
                ...saved,
                id: args.params.meetingId,
                status: "recording",
              };
              return saved;
            case "get_status":
              return { modelInstalled: true, captureAvailable: true };
            case "update_meeting":
              await new Promise((r) => setTimeout(r, 80));
              saved = { ...saved, ...args.params.changes };
              return saved;
            case "hide_window":
              state.__hiddenNotes = saved.notes;
              return;
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
    { meeting, settings: defaults },
  );
  await page.goto("/");
  await page.getByTitle("Include meeting audio from your computer").click();
  await page.evaluate(() => {
    (window as any).__intent("record");
    (window as any).__intent("record");
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).__starts))
    .toBe(1);
  expect(await page.evaluate(() => (window as any).__creates)).toBe(1);
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Save before hiding.");
  await page.evaluate(() => (window as any).__intent("hide"));
  await expect
    .poll(() => page.evaluate(() => (window as any).__hiddenNotes))
    .toBe("Save before hiding.");
});

// Shared fixture compatibility; production exclusively uses the lean protocol.
test.beforeEach(async ({ page }) => {
  await installLeanLibraryPort(page);
});
