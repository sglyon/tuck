import { expect, test } from "@playwright/test";
import { installLargeLibrary } from "./support/large-library";
const title = (page: any) =>
  page.getByRole("textbox", { name: "Conversation title" });

test("lean native startup, all/starred pages, full-library query, facets and source links", async ({
  page,
}) => {
  await installLargeLibrary(page);
  await page.goto("/");
  await expect(title(page)).toHaveValue("Conversation 000");
  expect(
    await page.evaluate(() =>
      (window as any).__calls
        .filter((c: any) => c.method === "get_meeting")
        .map((c: any) => c.params.meetingId),
    ),
  ).toEqual(["meeting-000"]);
  await expect(
    page.getByRole("button", { name: "All 135" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Load more conversations" }).click();
  await expect(page.locator(".meeting-item")).toHaveCount(100);
  await page.evaluate(() => {
    const w = window as any;
    w.__emit("segment", {
      meetingId: "meeting-000",
      segment: {
        id: "background",
        text: "Background evidence.",
        start: 3001,
        end: 3002,
        speakerId: "speaker-1",
      },
    });
  });
  // Background metadata refresh is throttled; it must retain older loaded rows.
  await expect
    .poll(
      () =>
        page.evaluate(
          () =>
            (window as any).__calls.filter(
              (c: any) => c.method === "library_overview",
            ).length,
        ),
      { timeout: 8000 },
    )
    .toBeGreaterThan(0);
  await expect(page.locator(".meeting-item")).toHaveCount(100);
  await expect(
    page.getByRole("button", { name: /Conversation 099/ }),
  ).toBeVisible();

  expect(
    await page.evaluate(() =>
      (window as any).__calls
        .filter((c: any) => c.method === "list_meetings")
        .every((c: any) => c.params.starred === undefined),
    ),
  ).toBe(true);
  await page.getByRole("button", { name: "Starred 2", exact: true }).click();
  await expect(page.locator(".meeting-item")).toHaveCount(2);
  await expect(
    page.getByRole("button", { name: /Conversation 119/ }),
  ).toBeVisible();
  await page.getByRole("button", { name: "All 135" }).click();
  await page
    .getByRole("textbox", { name: "Filter saved items" })
    .fill("obscure marker");
  await expect(page.locator(".meeting-item")).toHaveCount(1);
  await page.locator(".meeting-item").click();
  await expect(title(page)).toHaveValue("Conversation 119");
  await page.getByRole("textbox", { name: "Filter saved items" }).fill("");
  await page.getByText("Filter by folder, tag or person").click();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .selectOption("Off-page team");
  await expect(page.locator(".meeting-item")).toHaveCount(1);
  await page
    .getByRole("button", { name: "Search library", exact: true })
    .click();
  await expect(page.locator(".desk-scope")).toContainText("135 conversations");
  await page.getByText("Filter by folder, tag or person").click();
  await page.getByLabel("Filter by folder", {exact:true}).selectOption("Off-page team");
  await expect(page.locator(".desk-scope")).toContainText("1 conversation");
  await page
    .getByRole("textbox", { name: "Search or ask your library" })
    .fill("obscure");
  await page.getByRole("button", { name: "Search passages" }).click();
  await page.locator(".source-result").click();
  await expect(title(page)).toHaveValue("Conversation 119");
  await expect(page.locator("#segment-segment-119-0")).toContainText(
    "obscure marker",
  );
  expect(
    await page.evaluate(() =>
      (window as any).__calls.some((c: any) => c.method === "bootstrap"),
    ),
  ).toBe(false);
});

test("old detail replies stay with their owner; retry and stale pages preserve selection", async ({
  page,
}) => {
  await installLargeLibrary(page, { delayInitial: true });
  await page.goto("/");
  await expect(page.getByText("Opening conversation…")).toBeVisible();
  await page.getByRole("button", { name: /Conversation 001/ }).click();
  await expect(title(page)).toHaveValue("Conversation 001");
  await page.evaluate(() => (window as any).__releases["meeting-000"]());
  await expect(title(page)).toHaveValue("Conversation 001");
  await page.evaluate(() => {
    (window as any).__staleNextMore = true;
  });
  await page.getByRole("button", { name: "Load more conversations" }).click();
  await expect(page.locator(".meeting-item")).toHaveCount(50);
  await expect(title(page)).toHaveValue("Conversation 001");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("button", { name: "Load more conversations" }).click();
  await expect(page.locator(".meeting-item")).toHaveCount(100);
  await page.evaluate(() => {
    (window as any).__failDetails["meeting-002"] = true;
  });
  await page.getByRole("button", { name: /Conversation 002/ }).click();
  await expect(
    page.getByRole("heading", {
      name: "This conversation could not be opened.",
    }),
  ).toBeVisible();
  await page.evaluate(() => {
    delete (window as any).__failDetails["meeting-002"];
  });
  await page.getByRole("button", { name: "Try again", exact: true }).click();
  await expect(title(page)).toHaveValue("Conversation 002");
});

test("global paged task inbox opens off-page owners and supports omitted facet choices", async ({
  page,
}) => {
  await installLargeLibrary(page, { truncated: true });
  await page.goto("/");
  await expect(title(page)).toHaveValue("Conversation 000");
  await page.getByRole("button", { name: "Tasks 75", exact: true }).click();
  await expect(page.locator(".inbox-task")).toHaveCount(50);
  await page.getByRole("button", { name: /Load more tasks/ }).click();
  await expect(page.locator(".inbox-task")).toHaveCount(75);
  await page
    .getByRole("button", { name: "Conversation 119", exact: true })
    .click();
  await expect(title(page)).toHaveValue("Conversation 119");
  await page.getByText("Filter by folder, tag or person").click();
  await expect(page.getByText(/Showing the first 200 choices/)).toBeVisible();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .fill("Off-page team");
  await page.getByLabel("Filter by folder", { exact: true }).press("Enter");
  await expect(page.locator(".meeting-item")).toHaveCount(1);
  await page.getByRole("button", { name: "Tasks 75", exact: true }).click();
  await expect(page.locator(".inbox-task")).toHaveCount(50);
  await page.getByText("Filter by folder, tag or person").click();
  await page.getByLabel("Filter by folder",{exact:true}).fill("Off-page team");
  await page.getByLabel("Filter by folder",{exact:true}).press("Enter");
  await expect(page.locator(".inbox-task")).toHaveCount(1);
  await page.getByRole("checkbox", { name: "Complete Follow up 119" }).click();
  await expect(page.locator(".inbox-task")).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "You're up to date." }),
  ).toBeVisible();
});

test("detail cache evicts idle bodies while preserving edits and active recording events", async ({
  page,
}) => {
  await installLargeLibrary(page);
  await page.goto("/");
  await expect(title(page)).toHaveValue("Conversation 000");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Owned by conversation zero");
  for (let i = 1; i < 9; i++) {
    await page
      .getByRole("button", {
        name: new RegExp(`Conversation ${String(i).padStart(3, "0")}`),
      })
      .click();
    await expect(title(page)).toHaveValue(
      `Conversation ${String(i).padStart(3, "0")}`,
    );
  }
  await page.getByRole("button", { name: /Conversation 000/ }).click();
  await expect(title(page)).toHaveValue("Conversation 000");
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Owned by conversation zero");
  expect(
    await page.evaluate(() => (window as any).__saved["meeting-000"]),
  ).toBe("Owned by conversation zero");
  expect(
    await page.evaluate(
      () =>
        (window as any).__calls.filter(
          (c: any) =>
            c.method === "get_meeting" && c.params.meetingId === "meeting-000",
        ).length,
    ),
  ).toBe(2);
  await page.evaluate(() => {
    const w = window as any;
    w.__emit("recording_state", {
      meetingId: "meeting-000",
      state: "recording",
    });
    w.__emit("segment", {
      meetingId: "meeting-000",
      segment: {
        id: "live",
        text: "Incoming spoken detail.",
        start: 3001,
        end: 3003,
        speakerId: "speaker-1",
      },
    });
  });
  await page.getByRole("tab", { name: /Transcript/ }).click();
  await expect(page.locator("#segment-live")).toContainText(
    "Incoming spoken detail.",
  );
});

test("task completion refresh follows the latest Show completed selection", async ({
  page,
}) => {
  await installLargeLibrary(page);
  await page.goto("/");
  await expect(title(page)).toHaveValue("Conversation 000");
  await page.getByRole("button", { name: "Tasks 75", exact: true }).click();
  await expect(page.locator(".inbox-task")).toHaveCount(50);
  await page.evaluate(() => {
    (window as any).__holdTask = true;
  });
  await page
    .getByRole("checkbox", { name: "Complete Follow up 60", exact: true })
    .click();
  await page
    .getByRole("checkbox", { name: "Show completed", exact: true })
    .check();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__calls
            .filter((c: any) => c.method === "list_task_page")
            .at(-1)?.params.includeDone,
      ),
    )
    .toBe(true);
  await page.evaluate(() => (window as any).__releaseTask());
  await expect(
    page.getByRole("checkbox", { name: "Complete Follow up 60", exact: true }),
  ).toBeChecked();
  await expect(
    page.getByRole("checkbox", { name: "Show completed", exact: true }),
  ).toBeChecked();
  expect(
    await page.evaluate(
      () =>
        (window as any).__calls
          .filter((c: any) => c.method === "list_task_page")
          .at(-1).params.includeDone,
    ),
  ).toBe(true);
});

test("mobile expanded filters retain a usable, visible list retry area", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installLargeLibrary(page, { truncated: true });
  await page.goto("/");
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await page.getByText("Filter by folder, tag or person").click();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .scrollIntoViewIfNeeded();
  await page.evaluate(() => {
    (window as any).__failPage = true;
  });
  await page
    .getByRole("textbox", { name: "Filter saved items", exact: true })
    .fill("unavailable");
  await page
    .getByText("Library unavailable. Try again.", { exact: true })
    .waitFor();
  const retry = page
    .locator(".library-load-state")
    .getByRole("button", { name: "Try again", exact: true });
  await expect(retry).toBeInViewport();
  expect(
    (await page.locator(".meeting-list").boundingBox())!.height,
  ).toBeGreaterThanOrEqual(140);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.evaluate(() => {
    (window as any).__failPage = false;
  });
  await retry.click();
  await expect(
    page.getByText("No conversations found.", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    (window as any).__failPage = true;
  });
  await page
    .getByRole("textbox", { name: "Filter saved items", exact: true })
    .fill("another unavailable query");
  await page
    .getByText("Library unavailable. Try again.", { exact: true })
    .waitFor();
  await page.evaluate(() => {
    (window as any).__failPage = false;
  });
  await page
    .getByRole("textbox", { name: "Filter saved items", exact: true })
    .fill("");
  await expect(page.locator(".meeting-item")).toHaveCount(50);
});
