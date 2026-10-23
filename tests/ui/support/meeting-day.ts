import type { Page } from "@playwright/test";
import { defaults } from "../../../src/lib/bridge";
import { exampleMeeting } from "../../../src/lib/example";

/** Illustrative native contracts only: never accesses calendar, audio, accounts or OS permissions. */
export async function installMeetingDay(
  page: Page,
  options: {
    enabled?: boolean;
    connectable?: boolean;
    permission?: "granted" | "denied" | "unavailable" | "unknown";
    active?: boolean;
    stale?: boolean;
    failSave?: boolean;
    expiredImport?: boolean;
    slowDay?: boolean;
    failCalendarStatus?: boolean;
    failDayAfterFirst?: boolean;
  } = {},
) {
  const original = exampleMeeting();
  original.id = "fixture-existing";
  original.title = "Earlier conversation";
  original.isExample = false;
  original.notes = "";
  original.status = options.active ? "recording" : "complete";
  await page.addInitScript(
    ({ original, settings, options }) => {
      const w = window as any;
      const saved = sessionStorage.getItem("meeting-day-fixture");
      const db = saved
        ? JSON.parse(saved)
        : {
            meetings: [original],
            dismissed: false,
            enabled: !!options.enabled,
          };
      const persist = () =>
        sessionStorage.setItem("meeting-day-fixture", JSON.stringify(db));
      const start = new Date(Date.now() + 4 * 60000).toISOString();
      const event = {
        id: "ics:fixture-planning",
        source: "ics-import",
        title: "Product planning",
        start,
        end: new Date(Date.now() + 34 * 60000).toISOString(),
        joinUrl: "https://example.invalid/meeting-fixture",
        attendees: ["Fixture guest"],
        allDay: false,
      };
      const key = "fixture-occurrence";
      const callbacks = new Map<number, (e: any) => void>();
      const listeners = new Map<string, number[]>();
      let serial = 0;
      w.__meetingDayCalls = [];
      w.__meetingDayUnexpected = [];
      let calendarConnected = !!options.stale;
      let dayRequests = 0;
      let dayHeld = !!options.slowDay;
      w.__holdNextMeetingDay = () => {
        dayHeld = true;
        w.__releaseMeetingDay = null;
      };
      w.__meetingDaySaveFailed = !!options.failSave;
      const header = (m: any) => {
        const {
          notes,
          summary,
          segments,
          speakers,
          messages,
          bookmarks,
          tasks,
          ...rest
        } = m;
        return {
          ...rest,
          segmentCount: segments.length,
          speakerCount: speakers.length,
          messageCount: messages.length,
          bookmarkCount: bookmarks.length,
          taskCount: tasks?.length || 0,
          openTaskCount: 0,
          hasNotes: !!notes,
          hasSummary: !!summary,
        };
      };
      const status = {
        engineVersion: "illustrative-contract",
        platform: "test fixture",
        modelInstalled: false,
        modelDownloading: false,
        modelProgress: 0,
        speakerModelInstalled: false,
        activeMeetingId: options.active ? original.id : null,
        recordingState: options.active ? "recording" : "idle",
        captureAvailable: false,
      };
      const overview = () => ({
        meetingCount: db.meetings.length,
        starredCount: 0,
        openTaskCount: 0,
        folders: [],
        tags: [],
        people: [],
        facetsTruncated: { folders: false, tags: false, people: false },
        activeMeetingId: status.activeMeetingId,
      });
      const occurrence = () => ({
        event,
        occurrenceKey: key,
        meetingId: db.meetings.find((m: any) => m.calendarEvent)?.id || null,
      });
      const reminderStatus = () => ({
        available: options.permission !== "unavailable",
        permission: options.permission || "granted",
        enabled: db.enabled,
        running: true,
        message: null,
      });
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback(callback: (e: any) => void) {
          callbacks.set(++serial, callback);
          return serial;
        },
        async invoke(command: string, args: any) {
          if (command === "plugin:event|listen") {
            listeners.set(args.event, [
              ...(listeners.get(args.event) || []),
              args.handler,
            ]);
            return args.handler;
          }
          if (command === "plugin:event|unlisten") {
            listeners.set(
              args.event,
              (listeners.get(args.event) || []).filter((id) => id !== args.eventId),
            );
            callbacks.delete(args.eventId);
            return;
          }
          if (command !== "engine_call") {
            w.__meetingDayUnexpected.push(command);
            throw new Error(`Unsupported fixture command: ${command}`);
          }
          const { method, params: p = {} } = args;
          w.__meetingDayCalls.push({ method, params: p });
          if (method === "bootstrap_library")
            return {
              meetingPage: {
                meetings: db.meetings.map(header),
                nextCursor: null,
                total: db.meetings.length,
              },
              library: overview(),
              settings: { ...settings, meetingReminders: db.enabled },
              status,
              accounts: [],
            };
          if (method === "get_meeting")
            return structuredClone(
              db.meetings.find((m: any) => m.id === p.meetingId),
            );
          if (method === "list_meetings")
            return {
              meetings: db.meetings.map(header),
              nextCursor: null,
              total: db.meetings.length,
            };
          if (method === "library_overview") return overview();
          if (method === "get_live_coach") {
            const meeting = db.meetings.find((m: any) => m.id === p.meetingId);
            if (!meeting) throw new Error("Choose an available fixture meeting.");
            return {
              enabled: false,
              status: ["complete", "interrupted"].includes(meeting.status)
                ? "stopped"
                : "off",
              message: null,
              selectedAgentIds: [],
              suggestions: [],
            };
          }
          if (method === "get_meeting_day") {
            if (options.failDayAfterFirst && dayRequests++ > 0)
              throw new Error("Illustrative cached-day refresh failure");
            const snapshot = {
              events: options.expiredImport ? [] : [occurrence()],
              snapshots: [
                {
                  scope: "import",
                  refreshedAt: new Date().toISOString(),
                  expiresAt: new Date(
                    Date.now() + (options.expiredImport ? -1 : 7) * 86400000,
                  ).toISOString(),
                },
                ...(options.stale
                  ? [
                      {
                        scope: "device",
                        refreshedAt: new Date(
                          Date.now() - 10 * 60000,
                        ).toISOString(),
                        expiresAt: new Date(
                          Date.now() + 86400000,
                        ).toISOString(),
                      },
                    ]
                  : []),
              ],
              reminders:
                db.enabled &&
                !db.dismissed &&
                !options.active &&
                !options.expiredImport
                  ? [occurrence()]
                  : [],
              enabled: db.enabled,
              suppressed: !!options.active,
            };
            if (dayHeld) {
              dayHeld = false;
              await new Promise<void>((resolve) => {
                w.__releaseMeetingDay = resolve;
              });
            }
            return snapshot;
          }
          if (method === "calendar_status") {
            if (options.failCalendarStatus)
              throw new Error("Illustrative calendar capability error");
            return {
              available: !!options.connectable,
              connected: calendarConnected,
              source: "Illustrative fixture",
              message:
                "Device calendar is unavailable in this fixture. Import a local calendar file.",
            };
          }
          if (method === "meeting_reminder_status") return reminderStatus();
          if (method === "set_meeting_reminders") {
            db.enabled = p.enabled;
            persist();
            return reminderStatus();
          }
          if (method === "dismiss_meeting_reminder") {
            db.dismissed = true;
            persist();
            return null;
          }
          if (method === "prepare_calendar_meeting") {
            let prepared = db.meetings.find((m: any) => m.calendarEvent);
            if (!prepared) {
              prepared = {
                ...original,
                id: "fixture-prepared",
                title: event.title,
                status: "draft",
                notes: "",
                summary: "",
                segments: [],
                messages: [],
                speakers: [],
                bookmarks: [],
                tasks: [],
                calendarEvent: event,
                people: event.attendees,
              };
              db.meetings.unshift(prepared);
              persist();
            }
            return structuredClone(prepared);
          }
          if (method === "update_meeting") {
            if (w.__meetingDaySaveFailed)
              throw new Error("Illustrative save failure");
            const m = db.meetings.find((m: any) => m.id === p.id);
            Object.assign(m, p.changes);
            persist();
            return structuredClone(m);
          }
          if (method === "connect_calendar" && options.connectable) {
            calendarConnected = true;
            return { connected: true };
          }
          if (method === "calendar_events") return [event];
          if (method === "get_status") return status;
          w.__meetingDayUnexpected.push(method);
          throw new Error(
            `Unsupported fixture method: ${method}. This fixture cannot record, authenticate, access calendars or grant permissions.`,
          );
        },
      };
      w.__meetingDayState = () => structuredClone(db);
      w.__setFixtureRecording = (active: boolean) => {
        status.activeMeetingId = active ? original.id : null;
        original.status = active ? "recording" : "complete";
        for (const handler of listeners.get("engine-event") || [])
          callbacks.get(handler)?.({
            payload: {
              event: "recording_state",
              payload: { meetingId: original.id, state: original.status },
            },
          });
      };
    },
    { original, settings: defaults, options },
  );
}
