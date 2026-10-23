import { useCallback, useEffect, useRef, useState } from "react";
import { call, native } from "./bridge";
import type { CalendarEvent } from "./types";

export interface CalendarOccurrence {
  occurrenceKey: string;
  event: CalendarEvent;
  meetingId: string | null;
}
export interface MeetingDay {
  events: CalendarOccurrence[];
  snapshots: {
    scope: "device" | "import";
    refreshedAt: string;
    expiresAt: string;
  }[];
  reminders: CalendarOccurrence[];
  enabled: boolean;
  suppressed: boolean;
}
export interface CalendarStatus {
  available: boolean;
  connected: boolean;
  source: string;
  message?: string;
}
export interface ReminderStatus {
  available: boolean;
  permission:
    | "granted"
    | "denied"
    | "notDetermined"
    | "unavailable"
    | "unknown";
  enabled: boolean;
  running: boolean;
  message?: string | null;
}
const emptyDay: MeetingDay = {
  events: [],
  snapshots: [],
  reminders: [],
  enabled: false,
  suppressed: false,
};

export function useMeetingDay() {
  const [day, setDay] = useState<MeetingDay>(emptyDay);
  const [calendar, setCalendar] = useState<CalendarStatus | null>(null);
  const [reminder, setReminder] = useState<ReminderStatus | null>(null);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [now, setNow] = useState(Date.now());
  const flight = useRef<Promise<void> | null>(null);
  const capabilitiesWanted = useRef(false);
  const lastCapabilityCheck = useRef(0);
  const mounted = useRef(true);
  const actionPending = useRef(false);
  const refresh = useCallback(
    (capabilities: boolean | "force" | "fresh" = true): Promise<void> => {
      if (!native) return Promise.resolve();
      if (mounted.current) setNow(Date.now());
      // A mutation must read after an older poll settles. Coalescing onto that
      // poll could otherwise restore a dismissed reminder or an unprepared row.
      if (
        (capabilities === "force" || capabilities === "fresh") &&
        flight.current
      )
        return flight.current.then(() => refresh(capabilities));
      if (capabilities === true || capabilities === "force")
        capabilitiesWanted.current = true;
      if (flight.current) return flight.current;
      const task = (async () => {
        const failures: string[] = [];
        try {
          const nextDay = await call<MeetingDay>("get_meeting_day");
          if (mounted.current && nextDay) setDay(nextDay);
        } catch {
          failures.push("Upcoming meetings could not be refreshed.");
        }
        // Only the cached day is polled. Capability checks can launch native
        // helpers, so run them one at a time and at most once every 30 seconds.
        if (
          capabilitiesWanted.current &&
          (capabilities === "force" ||
            Date.now() - lastCapabilityCheck.current >= 30000)
        ) {
          capabilitiesWanted.current = false;
          lastCapabilityCheck.current = Date.now();
          try {
            const status = await call<CalendarStatus>("calendar_status");
            if (mounted.current && status) setCalendar(status);
          } catch {
            failures.push("Calendar connection status could not be checked.");
          }
          try {
            const status = await call<ReminderStatus>(
              "meeting_reminder_status",
            );
            if (mounted.current && status) setReminder(status);
          } catch {
            failures.push("Notification permission could not be checked.");
          }
        }
        capabilitiesWanted.current = false;
        if (mounted.current) setError(failures.join(" "));
      })();
      flight.current = task;
      void task.finally(() => {
        if (flight.current === task) flight.current = null;
      });
      return task;
    },
    [],
  );
  useEffect(() => {
    mounted.current = true;
    if (!native) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(false), 30000);
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      mounted.current = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);
  const run = async (
    action: () => Promise<void>,
    refreshCapabilities = false,
  ) => {
    if (actionPending.current) return;
    actionPending.current = true;
    setPending(true);
    try {
      await action();
      await refresh(refreshCapabilities ? "force" : "fresh");
    } finally {
      actionPending.current = false;
      setPending(false);
    }
  };
  const changeReminders = async (enabled: boolean) => {
    const previous = day.enabled;
    setDay((current) => ({ ...current, enabled }));
    try {
      const status = await call<ReminderStatus>("set_meeting_reminders", {
        enabled,
      });
      if (status) setReminder(status);
    } catch (e) {
      setDay((current) => ({ ...current, enabled: previous }));
      throw e;
    }
  };
  return {
    day,
    calendar,
    reminder,
    error,
    pending,
    now,
    refresh,
    run,
    changeReminders,
  };
}
export type MeetingDayController = ReturnType<typeof useMeetingDay>;

export function meetingTime(event: CalendarEvent) {
  return new Date(event.start).toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}
