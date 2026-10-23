import { test, expect } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page
    .getByRole("button", { name: "Open an example", exact: true })
    .click();
});
test("organization filters and source search lead back to exact spoken evidence", async ({
  page,
}) => {
  await page.getByText("Organize meeting", { exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting folder", exact: true })
    .fill("Customers");
  await page
    .getByRole("textbox", { name: "Meeting tags", exact: true })
    .fill("renewal, priority");
  await page
    .getByRole("textbox", { name: "Meeting people", exact: true })
    .fill("Maya");
  await page.getByRole("textbox", { name: "Conversation title" }).focus();
  await page
    .getByText("Filter by folder, tag or person", { exact: false })
    .click();
  await page
    .getByLabel("Filter by folder", { exact: true })
    .selectOption("Customers");
  await page
    .getByLabel("Filter by tag", { exact: true })
    .selectOption("renewal");
  await page
    .getByLabel("Filter by person", { exact: true })
    .selectOption("Maya");
  await page
    .getByRole("button", { name: "Ask your library", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Search or ask your library" })
    .fill("custom integration");
  await page
    .getByRole("button", { name: "Search passages", exact: true })
    .click();
  await expect(page.locator(".source-result")).toHaveCount(1);
  await page.locator(".source-result").click();
  await expect(page.locator(".transcript-segment.highlighted")).toContainText(
    "custom integration",
  );
});
test("meeting commitments persist and are actionable from the task inbox", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Tasks", exact: true }).click();
  await page
    .getByRole("textbox", { name: "New task", exact: true })
    .fill("Send the rollout plan");
  await page
    .getByRole("textbox", { name: "New task owner", exact: true })
    .fill("Maya");
  await page.getByRole("button", { name: "Add task", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "Complete Send the rollout plan" }),
  ).not.toBeChecked();
  await page
    .getByRole("navigation", { name: "Library filters" })
    .getByRole("button", { name: /^Tasks/ })
    .click();
  await expect(
    page.getByText("Send the rollout plan", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("checkbox", { name: "Complete Send the rollout plan" })
    .click();
  await expect(
    page.getByRole("heading", { name: "You're up to date." }),
  ).toBeVisible();
  await page.getByLabel("Show completed").check();
  await expect(
    page.getByRole("checkbox", { name: "Complete Send the rollout plan" }),
  ).toBeChecked();
  await page.reload();
  await page.getByRole("tab", { name: "Tasks", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "Complete Send the rollout plan" }),
  ).toBeChecked();
});
test("custom templates and writing recipes are editable and reachable in a meeting", async ({
  page,
}) => {
  await page.getByRole("button", { name: /^Settings/ }).click();
  await page.getByRole("tab", { name: "Writing", exact: true }).click();
  await page
    .getByRole("textbox", { name: "template name", exact: true })
    .fill("Customer review");
  await page
    .getByRole("textbox", { name: "template instructions", exact: true })
    .fill(
      "Summarize customer goals, objections and next steps with source citations.",
    );
  await page
    .getByRole("button", { name: "Save template", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Writing recipes", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "recipe name", exact: true })
    .fill("Follow-up email");
  await page
    .getByRole("textbox", { name: "recipe instructions", exact: true })
    .fill("Draft a brief follow-up email with agreed next steps.");
  await page.getByRole("button", { name: "Save recipe", exact: true }).click();
  await page.locator(".meeting-item").first().click();
  await page
    .getByLabel("Summary template")
    .selectOption({ label: "Customer review" });
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("button", { name: "Enhance notes", exact: true })
    .click();
  await page
    .getByLabel("Choose recipe")
    .selectOption({ label: "Follow-up email" });
  await expect(
    page.getByRole("textbox", { name: "Writing instruction" }),
  ).toHaveValue("Draft a brief follow-up email with agreed next steps.");
  await page
    .getByRole("button", { name: "Preview changes", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("desktop app");
});
test("editable recap survives view changes and reload independently of personal notes", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Edit recap", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Edit meeting recap", exact: true })
    .fill("A personal recap of the decision.");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          JSON.parse(localStorage.getItem("tuck-preview-v1")!).meetings[0]
            .summary,
      ),
    )
    .toBe("A personal recap of the decision.");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Keep the question open.");
  await expect(page.getByText("Saved locally", { exact: true })).toBeVisible();
  await page.reload();
  await expect(
    page.getByText("A personal recap of the decision.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Keep the question open.");
});
