import { expect, test } from "@playwright/test";
import { installMeetingDay } from "./support/meeting-day";
const next = (page: any) =>
  page.getByRole("region", { name: "Next meeting", exact: true });
const reminder = (page: any) =>
  page.getByRole("complementary", { name: "Meeting reminder" });
const title = (page: any) =>
  page.getByRole("textbox", { name: "Conversation title" });

test.afterEach(async ({ page }) => {
  const unexpected = await page.evaluate(
    () => (window as any).__meetingDayUnexpected || [],
  );
  expect(unexpected).toEqual([]);
});

test("next meeting atomically reuses notes, flushes drafts and returns to the right document", async ({
  page,
}) => {
  await installMeetingDay(page);
  await page.goto("/");
  await expect(
    next(page).getByText("Product planning", { exact: true }),
  ).toBeVisible();
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Keep the thought from the previous conversation.");
  await next(page)
    .getByRole("button", { name: "Prepare notes", exact: true })
    .click();
  await expect(title(page)).toHaveValue("Product planning");
  await expect(
    page.getByRole("tab", { name: "Notes", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Agenda that must survive reuse.");
  await next(page)
    .getByRole("button", { name: "Open notes", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Agenda that must survive reuse.");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any)
            .__meetingDayState()
            .meetings.find((m: any) => m.id === "fixture-prepared").notes,
      ),
    )
    .toBe("Agenda that must survive reuse.");
  const state = await page.evaluate(() => (window as any).__meetingDayState());
  expect(state.meetings).toHaveLength(2);
  expect(
    state.meetings.find((m: any) => m.id === "fixture-existing").notes,
  ).toContain("Keep the thought");
  expect(
    state.meetings.find((m: any) => m.id === "fixture-prepared").notes,
  ).toBe("Agenda that must survive reuse.");
  const methods = await page.evaluate(() =>
    (window as any).__meetingDayCalls.map((c: any) => c.method),
  );
  expect(
    methods.filter((m: string) => m === "prepare_calendar_meeting"),
  ).toHaveLength(2);
  expect(methods).not.toContain("create_meeting");
});

test("failed draft save keeps the current document open and preparation can retry", async ({
  page,
}) => {
  await installMeetingDay(page, { failSave: true });
  await page.goto("/");
  await expect(title(page)).toHaveValue("Earlier conversation");
  await page.getByRole("tab", { name: "Notes", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Meeting notes", exact: true })
    .fill("Do not lose this draft.");
  await next(page).getByRole("button", { name: "Prepare notes" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Save your pending edits",
  );
  await expect(title(page)).toHaveValue("Earlier conversation");
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toHaveValue("Do not lose this draft.");
  await page.evaluate(() => ((window as any).__meetingDaySaveFailed = false));
  await next(page).getByRole("button", { name: "Prepare notes" }).click();
  await expect(title(page)).toHaveValue("Product planning");
});

test("optional reminder enable handles denied notifications, durable dismiss and disabling", async ({
  page,
}) => {
  await installMeetingDay(page, { permission: "denied" });
  await page.goto("/");
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  const optIn = page.getByRole("checkbox", {
    name: "Remind me 5 minutes before",
  });
  await expect(optIn).not.toBeChecked();
  await expect(reminder(page)).toHaveCount(0);
  await optIn.check();
  await expect(next(page)).toContainText("System notifications are blocked");
  await expect(reminder(page)).toBeVisible();
  await reminder(page)
    .getByRole("button", { name: "Dismiss", exact: true })
    .click();
  await expect(reminder(page)).toHaveCount(0);
  await page.reload();
  await expect(title(page)).toHaveValue("Earlier conversation");
  await expect(reminder(page)).toHaveCount(0);
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await expect(optIn).toBeChecked();
  await optIn.uncheck();
  await expect(optIn).not.toBeChecked();
  expect(
    await page.evaluate(() =>
      (window as any).__meetingDayCalls
        .filter((c: any) => c.method === "set_meeting_reminders")
        .map((c: any) => c.params.enabled),
    ),
  ).toEqual([false]);
});

test("in-app reminder prepares notes and active capture suppresses interruption", async ({
  page,
}) => {
  await installMeetingDay(page, { enabled: true });
  await page.goto("/");
  await expect(reminder(page)).toBeVisible();
  await page.evaluate(() => (window as any).__setFixtureRecording(true));
  await expect(reminder(page)).toHaveCount(0);
  await page.evaluate(() => (window as any).__setFixtureRecording(false));
  await expect(reminder(page)).toBeVisible();
  await reminder(page)
    .getByRole("button", { name: "Prepare notes", exact: true })
    .click();
  await expect(title(page)).toHaveValue("Product planning");
  await expect(reminder(page)).toHaveCount(0);
});

test("stale imported/calendar state and unavailable notifications remain actionable on compact screens", async ({
  page,
}) => {
  await installMeetingDay(page, {
    enabled: true,
    permission: "unavailable",
    stale: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await expect(next(page)).toBeVisible();
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await expect(next(page)).toContainText("Refresh your device calendar");
  await expect(next(page)).toContainText("Imported snapshot");
  await expect(next(page)).toContainText(
    "System notifications are unavailable",
  );
  await expect(
    next(page).getByRole("button", { name: "Refresh calendar", exact: true }),
  ).toBeVisible();
  await next(page)
    .getByRole("button", { name: "Prepare notes", exact: true })
    .click();
  await expect(title(page)).toHaveValue("Product planning");
  await expect(
    page.getByRole("textbox", { name: "Meeting notes", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
});

test("an expired import asks for an updated snapshot rather than claiming the calendar is empty", async ({
  page,
}) => {
  await installMeetingDay(page, { enabled: true, expiredImport: true });
  await page.goto("/");
  await expect(next(page)).toContainText("Import an updated calendar");
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await expect(next(page)).toContainText("Your imported snapshot has expired");
  await expect(
    next(page).getByRole("button", { name: "Replace .ics snapshot" }),
  ).toBeVisible();
  await expect(reminder(page)).toHaveCount(0);
});

test("slow cached polls coalesce focus requests and a capability failure keeps imported meetings usable", async ({
  page,
}) => {
  await page.clock.install();
  await installMeetingDay(page, { slowDay: true, failCalendarStatus: true });
  await page.goto("/");
  await expect(title(page)).toHaveValue("Earlier conversation");
  await page.evaluate(() => {
    for (let i = 0; i < 8; i++) window.dispatchEvent(new Event("focus"));
  });
  await page.clock.fastForward(31000);
  expect(
    await page.evaluate(
      () =>
        (window as any).__meetingDayCalls.filter(
          (c: any) => c.method === "get_meeting_day",
        ).length,
    ),
  ).toBe(1);
  await page.evaluate(() => (window as any).__releaseMeetingDay());
  await expect(
    next(page).getByText("Product planning", { exact: true }),
  ).toBeVisible();
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await expect(next(page)).toContainText(
    "Calendar connection status could not be checked",
  );
  await page.evaluate(() => {
    for (let i = 0; i < 8; i++) window.dispatchEvent(new Event("focus"));
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__meetingDayCalls.filter(
            (c: any) => c.method === "get_meeting_day",
          ).length,
      ),
    )
    .toBe(2);
  expect(
    await page.evaluate(
      () =>
        (window as any).__meetingDayCalls.filter(
          (c: any) => c.method === "calendar_status",
        ).length,
    ),
  ).toBe(1);
  await next(page).getByRole("button", { name: "Prepare notes" }).click();
  await expect(title(page)).toHaveValue("Product planning");
});

test("an explicit calendar connection immediately refreshes capability state within the passive rate limit", async ({
  page,
}) => {
  await installMeetingDay(page, { connectable: true });
  await page.goto("/");
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await next(page)
    .getByRole("button", { name: "Connect device calendar" })
    .click();
  await expect(
    next(page).getByRole("button", { name: "Refresh calendar", exact: true }),
  ).toBeVisible();
  await expect(
    next(page).getByRole("button", { name: "Connect device calendar" }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as any).__meetingDayCalls.filter(
          (c: any) => c.method === "calendar_status",
        ).length,
    ),
  ).toBe(2);
});

for (const action of ["prepare", "dismiss", "connect"] as const) {
  test(`a pre-action poll cannot replace fresh ${action} state`, async ({
    page,
  }) => {
    await installMeetingDay(page, {
      enabled: action !== "connect",
      connectable: action === "connect",
    });
    await page.goto("/");
    await expect(title(page)).toHaveValue("Earlier conversation");
    if (action === "connect") {
      await next(page)
        .getByText("Calendar & reminders", { exact: true })
        .click();
      await expect(
        next(page).getByRole("button", { name: "Connect device calendar" }),
      ).toBeVisible();
    } else await expect(reminder(page)).toBeVisible();
    await page.evaluate(() => {
      (window as any).__holdNextMeetingDay();
      window.dispatchEvent(new Event("focus"));
    });
    await expect
      .poll(() =>
        page.evaluate(() => typeof (window as any).__releaseMeetingDay),
      )
      .toBe("function");
    if (action === "prepare") {
      await next(page)
        .getByRole("button", { name: "Prepare notes", exact: true })
        .click();
      await expect(title(page)).toHaveValue("Product planning");
    } else if (action === "dismiss") {
      await reminder(page)
        .getByRole("button", { name: "Dismiss", exact: true })
        .click();
      await expect
        .poll(() =>
          page.evaluate(() => (window as any).__meetingDayState().dismissed),
        )
        .toBe(true);
    } else {
      await next(page)
        .getByRole("button", { name: "Connect device calendar" })
        .click();
      await expect
        .poll(() =>
          page.evaluate(() =>
            (window as any).__meetingDayCalls.some(
              (c: any) => c.method === "calendar_events",
            ),
          ),
        )
        .toBe(true);
    }
    await page.evaluate(() => (window as any).__releaseMeetingDay());
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            (window as any).__meetingDayCalls.filter(
              (c: any) => c.method === "get_meeting_day",
            ).length,
        ),
      )
      .toBe(3);
    if (action === "prepare")
      await expect(
        next(page).getByRole("button", { name: "Open notes", exact: true }),
      ).toBeEnabled();
    else if (action === "dismiss") await expect(reminder(page)).toHaveCount(0);
    else {
      await expect(
        next(page).getByRole("button", {
          name: "Refresh calendar",
          exact: true,
        }),
      ).toBeEnabled();
      await expect(
        next(page).getByRole("button", { name: "Connect device calendar" }),
      ).toHaveCount(0);
    }
  });
}

test("a cached reminder expires locally even when the next day refresh fails", async ({
  page,
}) => {
  await page.clock.install();
  await installMeetingDay(page, { enabled: true, failDayAfterFirst: true });
  await page.goto("/");
  await expect(reminder(page)).toBeVisible();
  await page.clock.fastForward(6 * 60000);
  await expect(reminder(page)).toHaveCount(0);
});

test("browser calendar preview makes its limits explicit", async ({ page }) => {
  await page.goto("/");
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await expect(next(page)).toContainText(
    "browser preview never reads your calendar or sends notifications",
  );
  await expect(
    page.getByRole("checkbox", { name: "Remind me 5 minutes before" }),
  ).toHaveCount(0);
});

test("visual evidence: one desktop and compact fixture capture batch", async ({
  page,
}) => {
  await installMeetingDay(page, {
    enabled: true,
    permission: "denied",
    stale: true,
  });
  await page.goto("/");
  await expect(title(page)).toHaveValue("Earlier conversation");
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await page.screenshot({
    path: ".impeccable/review/meeting-day-desktop.png",
    fullPage: true,
    animations: "disabled",
  });
  await page
    .getByRole("checkbox", { name: "Remind me 5 minutes before" })
    .scrollIntoViewIfNeeded();
  await page.screenshot({
    path: ".impeccable/review/meeting-day-desktop-preferences.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.setViewportSize({ width: 390, height: 844 });
  // Reload after resizing so Chromium commits all fixed-pane paint layers.
  await page.reload();
  await expect(title(page)).toHaveValue("Earlier conversation");
  await expect(
    page.getByRole("button", { name: "Library", exact: true }),
  ).toBeInViewport();
  await page.screenshot({
    path: ".impeccable/review/meeting-day-compact-reminder.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Library", exact: true }).click();
  await page.screenshot({
    path: ".impeccable/review/meeting-day-compact-library.png",
    fullPage: true,
    animations: "disabled",
  });
  await next(page).getByText("Calendar & reminders", { exact: true }).click();
  await page
    .getByRole("checkbox", { name: "Remind me 5 minutes before" })
    .scrollIntoViewIfNeeded();
  await page.screenshot({
    path: ".impeccable/review/meeting-day-compact-preferences.png",
    fullPage: true,
    animations: "disabled",
  });
});
