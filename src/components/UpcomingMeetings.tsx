import {
  CalendarDays,
  ChevronRight,
  LoaderCircle,
  RefreshCw,
  Upload,
} from "lucide-react";
import { call, native } from "../lib/bridge";
import {
  meetingTime,
  type CalendarOccurrence,
  type MeetingDayController,
} from "../lib/useMeetingDay";

interface Props {
  controller: MeetingDayController;
  onPrepare: (occurrence: CalendarOccurrence) => Promise<void>;
  onRemindersChanged: (enabled: boolean) => void;
  notify: (text: string, error?: boolean) => void;
}
export default function UpcomingMeetings({
  controller,
  onPrepare,
  onRemindersChanged,
  notify,
}: Props) {
  const { day, calendar, reminder, pending, error, now, run } = controller;
  const events = day.events.filter(
    ({ event }) =>
      !event.allDay &&
      Date.parse(event.end) > now &&
      Date.parse(event.start) <= now + 7 * 86400000,
  );
  const next = events[0];
  const imported = day.snapshots.find(
    (snapshot) => snapshot.scope === "import",
  );
  const importExpired = imported && Date.parse(imported.expiresAt) <= now;
  const device = day.snapshots.find((snapshot) => snapshot.scope === "device");
  const stale = device && now - Date.parse(device.refreshedAt) > 5 * 60000;
  const act = (action: () => Promise<void>, refreshCapabilities = false) =>
    void run(action, refreshCapabilities).catch((e) =>
      notify(e instanceof Error ? e.message : String(e), true),
    );
  const refreshCalendar = async () => {
    const from = new Date();
    await call("calendar_events", {
      from: from.toISOString(),
      to: new Date(from.getTime() + 7 * 86400000).toISOString(),
    });
  };
  return (
    <section className="upcoming-meetings" aria-label="Next meeting">
      <h2>
        <CalendarDays size={14} />
        Next meeting
      </h2>
      {next ? (
        <div className="upcoming-event">
          <strong>{next.event.title}</strong>
          <span>{meetingTime(next.event)}</span>
          <button
            className="button primary small"
            disabled={pending}
            onClick={() => act(() => onPrepare(next))}
          >
            {next.meetingId ? "Open notes" : "Prepare notes"}
          </button>
        </div>
      ) : (
        <p>
          {native
            ? importExpired
              ? "Import an updated calendar to see your next meeting."
              : "No upcoming meetings to prepare."
            : "Your next meeting, ready to prepare."}
        </p>
      )}
      <details className="calendar-options">
        <summary>
          Calendar & reminders
          <ChevronRight size={13} />
        </summary>
        {!native ? (
          <p>
            Connect a calendar and enable reminders in the desktop app. This
            browser preview never reads your calendar or sends notifications.
          </p>
        ) : (
          <>
            {error && (
              <div className="calendar-error" role="status">
                <p>{error}</p>
                <button
                  className="text-link"
                  disabled={pending}
                  onClick={() => act(async () => {}, true)}
                >
                  Try again
                </button>
              </div>
            )}
            {!calendar?.connected && (
              <p>
                {calendar?.message ||
                  "Connect your device calendar or import a calendar file."}
              </p>
            )}
            {calendar?.connected && (
              <p>
                Device calendar
                {device
                  ? ` · refreshed ${new Date(device.refreshedAt).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`
                  : ""}
                .
              </p>
            )}
            {stale && (
              <p>Refresh your device calendar to keep reminders current.</p>
            )}
            {!calendar?.connected && calendar?.available && (
              <button
                className="text-link"
                disabled={pending}
                onClick={() =>
                  act(async () => {
                    await call("connect_calendar");
                    await refreshCalendar();
                  }, true)
                }
              >
                Connect device calendar
              </button>
            )}
            {calendar?.connected && (
              <button
                className="text-link"
                disabled={pending}
                onClick={() => act(refreshCalendar, true)}
              >
                <RefreshCw size={12} />
                Refresh calendar
              </button>
            )}
            <button
              className="text-link"
              disabled={pending}
              onClick={() =>
                act(async () => {
                  const { open } = await import("@tauri-apps/plugin-dialog");
                  const path = await open({
                    multiple: false,
                    filters: [{ name: "Calendar file", extensions: ["ics"] }],
                  });
                  if (typeof path === "string")
                    await call("import_calendar", { path });
                })
              }
            >
              <Upload size={12} />
              {imported ? "Replace .ics snapshot" : "Import .ics file"}
            </button>
            {imported && (
              <p>
                {importExpired
                  ? "Your imported snapshot has expired. Import a current .ics file to update your meetings."
                  : `Imported snapshot · ${new Date(imported.refreshedAt).toLocaleDateString()}. Reimport the file when your schedule changes.`}
              </p>
            )}
            <p className="quiet-note">
              Calendar files do not update automatically; recurring rules are
              not expanded.
            </p>
            <div className="meeting-reminder-setting">
              <label>
                <input
                  type="checkbox"
                  checked={day.enabled}
                  disabled={pending || !reminder}
                  onChange={(e) => {
                    const enabled = e.currentTarget.checked;
                    act(async () => {
                      await controller.changeReminders(enabled);
                      onRemindersChanged(enabled);
                    });
                  }}
                />
                Remind me 5 minutes before
              </label>
              <p>
                {!day.enabled
                  ? "Optional, for events with guests or a meeting link. Nothing records automatically."
                  : reminder?.permission === "denied"
                    ? "System notifications are blocked. Allow Tuck in system notification settings; reminders still appear here while Tuck is open."
                    : reminder?.permission === "unavailable" ||
                        !reminder?.available
                      ? "System notifications are unavailable. Reminders appear here while Tuck is open."
                      : reminder?.permission === "unknown"
                        ? "Reminders appear here. Your system controls notification delivery; Tuck cannot verify whether it is allowed."
                        : reminder?.permission === "granted"
                          ? "Reminders appear here and use system notifications while Tuck is running."
                          : "Reminders appear here while Tuck is open. System notification permission has not been granted."}
              </p>
            </div>
            {events.length > 1 && (
              <details className="later-meetings">
                <summary>
                  {events.length - 1} more this week
                  <ChevronRight size={13} />
                </summary>
                {events.slice(1, 6).map((item) => (
                  <div className="upcoming-event" key={item.occurrenceKey}>
                    <strong>{item.event.title}</strong>
                    <span>{meetingTime(item.event)}</span>
                    <button
                      className="text-link"
                      disabled={pending}
                      onClick={() => act(() => onPrepare(item))}
                    >
                      {item.meetingId ? "Open notes" : "Prepare notes"}
                    </button>
                  </div>
                ))}
                {events.length > 6 && (
                  <p>More meetings appear as the week progresses.</p>
                )}
              </details>
            )}
          </>
        )}
      </details>
      {pending && (
        <span className="calendar-working" role="status">
          <LoaderCircle size={12} className="spin" />
          Updating…
        </span>
      )}
    </section>
  );
}

export function MeetingReminder({
  controller,
  onPrepare,
  notify,
  active,
}: Omit<Props, "onRemindersChanged"> & { active: boolean }) {
  const { day, pending, now, run } = controller;
  const due =
    !active && !day.suppressed && day.enabled
      ? day.reminders.find(({ event }) => {
          const startsIn = Date.parse(event.start) - now;
          return (
            !event.allDay &&
            startsIn >= -60000 &&
            startsIn <= 300000 &&
            Date.parse(event.end) > now
          );
        })
      : null;
  if (!due) return null;
  const act = (action: () => Promise<void>, refreshCapabilities = false) =>
    void run(action, refreshCapabilities).catch((e) =>
      notify(e instanceof Error ? e.message : String(e), true),
    );
  return (
    <aside
      className="meeting-reminder"
      aria-label="Meeting reminder"
      aria-live="polite"
    >
      <CalendarDays size={18} />
      <div>
        <h2>{due.event.title}</h2>
        <p>Starts {meetingTime(due.event)}</p>
      </div>
      <div className="meeting-reminder-actions">
        <button
          className="button primary small"
          disabled={pending}
          onClick={() =>
            act(async () => {
              await onPrepare(due);
              await call("dismiss_meeting_reminder", {
                occurrenceKey: due.occurrenceKey,
              });
            })
          }
        >
          {due.meetingId ? "Open notes" : "Prepare notes"}
        </button>
        <button
          className="button secondary small"
          disabled={pending}
          onClick={() =>
            act(async () => {
              await call("dismiss_meeting_reminder", {
                occurrenceKey: due.occurrenceKey,
              });
            })
          }
        >
          Dismiss
        </button>
      </div>
    </aside>
  );
}
