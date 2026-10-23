import { test, expect } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.reload();
});
test("library begins empty and an example only loads on request", async ({
  page,
}) => {
  await expect(
    page.getByRole("heading", { name: /Stay in the/ }),
  ).toBeVisible();
  await expect(
    page.getByText("Northstar / discovery call", { exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await expect(
    page.getByText("This is an illustrative conversation.", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", {
      name: "A clearer handoff, without another tool",
    }),
  ).toBeVisible();
});
test("notes survive changing views and reopening the workspace", async ({
  page,
}) => {
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Remember the customer commitments.");
  await expect(page.getByText("Saved locally", { exact: true })).toBeVisible();
  await page.reload();
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Remember the customer commitments.");
});
test("citations reveal the right transcript and speakers can be corrected", async ({
  page,
}) => {
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await page.getByRole("button", { name: "01:19", exact: true }).click();
  await expect(page.locator(".transcript-segment.highlighted")).toContainText(
    "existing Notion database",
  );
  await page
    .locator(".speaker-legend")
    .getByRole("button", { name: "Eli Brooks" })
    .click();
  await page.getByRole("textbox", { name: "Speaker name" }).fill("Eli Rivera");
  await page.getByRole("button", { name: "Save name", exact: true }).click();
  await expect(
    page.locator(".speaker-legend").getByRole("button", { name: "Eli Rivera" }),
  ).toBeVisible();
  await page
    .getByRole("textbox", { name: "Search this transcript" })
    .fill("custom integration");
  await expect(page.locator(".transcript-segment")).toHaveCount(1);
});
test("exports use real data and deleting a meeting requires confirmation", async ({
  page,
}) => {
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Export conversation", exact: true })
    .click();
  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("menuitem", { name: "Markdown", exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("Northstar  discovery call.md");
  await page.getByRole("button", { name: "More conversation actions" }).click();
  await page.getByRole("menuitem", { name: "Delete conversation" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "Keep conversation" }).click();
  await expect(
    page.getByRole("textbox", { name: "Conversation title" }),
  ).toBeVisible();
});
test("preview never accepts or persists credentials", async ({ page }) => {
  await page.getByRole("button", { name: /^Settings/ }).click();
  await page.getByRole("button", { name: "OpenAI API", exact: false }).click();
  await page
    .getByRole("textbox", { name: "API key", exact: false })
    .fill("sk-private-test-key");
  await page.getByRole("button", { name: "Save key", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("desktop app");
  const stored = await page.evaluate(() => JSON.stringify(localStorage));
  expect(stored).not.toContain("sk-private-test-key");
});
test("mobile uses one pane with a reachable assistant and no horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Workspace panes" })
    .getByRole("button", { name: "Ask Tuck" })
    .click();
  await expect(
    page.getByRole("heading", { name: "Ask Tuck", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation", { name: "Workspace panes" })
    .getByRole("button", { name: "Library" })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Search conversations" }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
});
