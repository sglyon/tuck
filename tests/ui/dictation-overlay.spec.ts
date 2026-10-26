import { test, expect, type Page } from "@playwright/test";

async function overlay(page: Page, held = false) {
  await page.setViewportSize({ width: 320, height: 76 });
  await page.addInitScript(({ held }) => {
    const w = window as any;
    const callbacks = new Map<number, any>();
    const listeners = new Set<number>();
    let serial = 0;
    const initial = { state: "listening", shortcut: "F20", shortcutAvailable: true };
    w.__overlayCalls = [];
    w.__overlayStatus = (state: string) => {
      for (const id of listeners) callbacks.get(id)?.({ payload: { event: "global_dictation_updated", payload: { ...initial, state, message: "Private dictated text must never appear here." } } });
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    w.__TAURI_INTERNALS__ = {
      transformCallback(callback: any) { callbacks.set(++serial, callback); return serial; },
      async invoke(command: string, args: any) {
        if (command === "plugin:event|listen") { listeners.add(args.handler); return args.handler; }
        if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
        if (command !== "engine_call" || args.method !== "global_dictation_status") throw new Error(`Unexpected overlay call: ${command}/${args.method}`);
        w.__overlayCalls.push(args.method);
        if (held) return new Promise(resolve => { w.__releaseOverlayStatus = () => resolve(initial); });
        return initial;
      },
    };
  }, { held });
  await page.goto("/?dictation-overlay");
}

test("floating feedback shows phases without mounting the workspace or capturing audio", async ({ page }) => {
  await overlay(page);
  await expect(page.getByRole("status")).toContainText("Listening");
  await expect(page.getByText("F20 to finish")).toBeVisible();
  await expect(page.getByRole("button")).toHaveCount(0);
  await expect(page.getByRole("navigation")).toHaveCount(0);
  for (const [state, title] of [["starting", "Getting ready…"], ["processing", "Transcribing…"], ["copied", "Copied to clipboard"], ["paste_sent", "Paste sent"], ["error", "Dictation needs attention"]]) {
    await page.evaluate(state => (window as any).__overlayStatus(state), state);
    await expect(page.getByRole("status")).toContainText(title);
    await expect(page.getByText("Private dictated text", { exact: false })).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth && document.documentElement.scrollHeight <= innerHeight)).toBe(true);
  }
  await page.screenshot({ path: ".impeccable/review/dictation-overlay-error.png" });
  await page.evaluate(() => (window as any).__overlayStatus("listening"));
  await page.screenshot({ path: ".impeccable/review/dictation-overlay-listening.png" });
  expect(await page.evaluate(() => (window as any).__overlayCalls.every((method: string) => method === "global_dictation_status"))).toBe(true);
  await page.evaluate(() => (window as any).__overlayStatus("idle"));
  await expect(page.getByRole("status")).toHaveCount(0);
});

test("late initial status cannot overwrite a newer overlay event", async ({ page }) => {
  await overlay(page, true);
  await expect.poll(async () => page.evaluate(() => typeof (window as any).__releaseOverlayStatus)).toBe("function");
  await page.evaluate(() => { (window as any).__overlayStatus("processing"); (window as any).__releaseOverlayStatus(); });
  await expect(page.getByRole("status")).toContainText("Transcribing…");
});
