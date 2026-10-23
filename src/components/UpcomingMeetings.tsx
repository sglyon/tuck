import { useEffect, useState } from "react";
import {
  CalendarDays,
  ChevronRight,
  LoaderCircle,
  Plus,
  Upload,
} from "lucide-react";
import { call, native, openExternal } from "../lib/bridge";
import type { CalendarEvent, Meeting } from "../lib/types";
type Status = {
  available: boolean;
  connected: boolean;
  source: string;
  message?: string;
};
export default function UpcomingMeetings({
  merge,
  choose,
  notify,
}: {
  merge: (m: Meeting) => void;
  choose: (id: string) => void;
  notify: (text: string, error?: boolean) => void;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const load = async () => {
    const s = await call<Status>("calendar_status");
    setStatus(s);
    if (s.connected) {
      const from = new Date();
      const to = new Date(from.getTime() + 7 * 86400000);
      setEvents(
        await call<CalendarEvent[]>("calendar_events", {
          from: from.toISOString(),
          to: to.toISOString(),
        }),
      );
    }
  };
  useEffect(() => {
    if (native) void load().catch(() => {});
  }, []);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
    } finally {
      setBusy(false);
    }
  };
  return (
    <details
      className="upcoming-meetings"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary>
        <CalendarDays size={14} />
        Upcoming
        <ChevronRight size={13} />
      </summary>
      <div>
        {!native ? (
          <p>
            Connect your calendar in the desktop app. Nothing records
            automatically.
          </p>
        ) : (
          <>
            <p>
              {status?.connected
                ? `${status.source} · next 7 days`
                : status?.message ||
                  "Bring upcoming meetings into your workspace."}
            </p>
            {!status?.connected && status?.available && (
              <button
                className="text-link"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await call("connect_calendar");
                    await load();
                  })
                }
              >
                Connect device calendar
              </button>
            )}
            <button
              className="text-link"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const { open } = await import("@tauri-apps/plugin-dialog");
                  const path = await open({
                    multiple: false,
                    filters: [{ name: "Calendar file", extensions: ["ics"] }],
                  });
                  if (typeof path === "string")
                    setEvents(
                      await call<CalendarEvent[]>("import_calendar", { path }),
                    );
                })
              }
            >
              <Upload size={12} />
              Import .ics file
            </button>
            {status?.connected && (
              <button
                className="text-link"
                disabled={busy}
                onClick={() => void run(load)}
              >
                Refresh calendar
              </button>
            )}
            <p className="quiet-note">
              Calendar files are imported snapshots; recurring rules are not
              expanded. Direct Google / Outlook sign-in is not configured.
            </p>
          </>
        )}
        {busy && <LoaderCircle className="spin" size={14} />}{" "}
        {!events.length && status?.connected && (
          <p>No upcoming events this week.</p>
        )}
        {events.map((event) => (
          <div className="upcoming-event" key={event.id}>
            <strong>{event.title}</strong>
            <span>
              {new Date(event.start).toLocaleString(undefined, {
                weekday: "short",
                hour: "numeric",
                minute: "2-digit",
              })}
            </span>
            <button
              className="text-link"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const m = await call<Meeting>("create_meeting", {
                    title: event.title,
                  });
                  const prepared = await call<Meeting>("update_meeting", {
                    id: m.id,
                    changes: { calendarEvent: event, people: event.attendees },
                  });
                  merge(prepared);
                  choose(m.id);
                  notify(
                    "Meeting prepared. Start recording when you're ready.",
                  );
                })
              }
            >
              <Plus size={12} />
              Prepare notes
            </button>
            {event.joinUrl && (
              <button
                className="text-link"
                onClick={() => void openExternal(event.joinUrl)}
              >
                Join meeting
              </button>
            )}
          </div>
        ))}
      </div>
    </details>
  );
}
