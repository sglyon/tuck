import type { Page } from "@playwright/test";
import { defaults } from "../../../src/lib/bridge";
import { exampleMeeting } from "../../../src/lib/example";

/** Synthetic native API fixture: headers at startup, complete bodies only on explicit get. */
export async function installLargeLibrary(
  page: Page,
  options: {
    delayInitial?: boolean;
    truncated?: boolean;
    illustrative?: boolean;
  } = {},
) {
  await page.addInitScript(
    ({ base, settings, options }) => {
      const state = window as unknown as Record<string, any>;
      const callbacks = new Map<number, (event: any) => void>();
      let cb = 0;
      const listeners = new Set<number>();
      state.__calls = [];
      state.__saved = {};
      state.__gates = {};
      state.__releases = {};
      state.__failDetails = {};
      state.__staleNextMore = false;
      state.__failPage = false;
      if (options.delayInitial) state.__gates["meeting-000"] = true;
      const meta = (i: number) => ({
        id: `meeting-${String(i).padStart(3, "0")}`,
        title: `${options.illustrative ? "Example fixture / " : ""}Conversation ${String(i).padStart(3, "0")}`,
        createdAt: new Date(Date.UTC(2026, 9, 23) - i * 3600000).toISOString(),
        duration: 3600,
        status: "complete",
        template: "meeting",
        starred: i === 0 || i === 119,
        isExample: !!options.illustrative,
        notionUrl: null,
        notionStatus: "local",
        folder: i === 119 ? "Off-page team" : "General",
        tags: i === 119 ? ["distant"] : [],
        people: i === 119 ? ["Person 119"] : [],
        calendarEvent: null,
        recoveredFrom: null,
        segmentCount: 1000,
        speakerCount: 1,
        messageCount: 0,
        bookmarkCount: 0,
        taskCount: i >= 60 ? 1 : 0,
        openTaskCount: i >= 60 && !state.__saved[`done-${i}`] ? 1 : 0,
        hasNotes: !!state.__saved[`meeting-${String(i).padStart(3, "0")}`],
        hasSummary: false,
      });
      const headers = () => Array.from({ length: 135 }, (_, i) => meta(i));
      const overview = () => ({
        meetingCount: 135,
        starredCount: 2,
        openTaskCount: headers().reduce((n, m) => n + m.openTaskCount, 0),
        folders: options.truncated
          ? Array.from({ length: 200 }, (_, i) => `Folder ${i}`)
          : ["General", "Off-page team"],
        tags: ["distant"],
        people: ["Person 119"],
        facetsTruncated: {
          folders: !!options.truncated,
          tags: false,
          people: false,
        },
        activeMeetingId: null,
      });
      const status = {
        engineVersion: "large-library-fixture",
        platform: "fixture",
        modelInstalled: true,
        modelDownloading: false,
        modelProgress: 0,
        speakerModelInstalled: false,
        activeMeetingId: null,
        recordingState: "idle",
        captureAvailable: true,
      };
      const full = (id: string) => {
        const i = Number(id.split("-")[1]),
          m = meta(i);
        return {
          ...base,
          ...m,
          notes: state.__saved[id] || "",
          summary: "",
          tasks:
            i >= 60
              ? [
                  {
                    id: "shared-task-id",
                    title: `Follow up ${i}`,
                    owner: "",
                    dueDate: "",
                    done: !!state.__saved[`done-${i}`],
                    createdAt: m.createdAt,
                  },
                ]
              : [],
          messages: [],
          bookmarks: [],
          speakers: [base.speakers[0]],
          segments: Array.from({ length: 1000 }, (_, j) => ({
            ...base.segments[0],
            id: `segment-${i}-${j}`,
            text:
              j === 0
                ? `Evidence for conversation ${i}. ${i === 119 ? "obscure marker" : ""}`
                : `Passage ${j}`,
            start: j * 3,
            end: j * 3 + 2,
          })),
        };
      };
      const scoped = (p: any) =>
        headers().filter(
          (m) =>
            (p.starred === undefined || m.starred === p.starred) &&
            (!p.scope?.folder || m.folder === p.scope.folder) &&
            (!p.scope?.tags?.length ||
              p.scope.tags.every((t: string) => m.tags.includes(t))) &&
            (!p.scope?.people?.length ||
              p.scope.people.every((t: string) => m.people.includes(t))) &&
            (!p.query ||
              m.title.toLowerCase().includes(p.query.toLowerCase()) ||
              (m.id === "meeting-119" && p.query === "obscure marker")),
        );
      const meetingPage = (p: any) => {
        if (state.__failPage)
          throw new Error("Library unavailable. Try again.");
        if (p.cursor && state.__staleNextMore) {
          state.__staleNextMore = false;
          throw { code: "cursor_stale", message: "Library changed." };
        }
        const all = scoped(p),
          offset = Number(p.cursor || 0),
          limit = p.limit || 50;
        return {
          meetings: all.slice(offset, offset + limit),
          total: all.length,
          nextCursor:
            offset + limit < all.length ? String(offset + limit) : null,
        };
      };
      state.__emit = (event: string, payload: any) =>
        listeners.forEach((handler) =>
          callbacks.get(handler)?.({
            event: "engine-event",
            id: handler,
            payload: { event, payload },
          }),
        );
      state.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      state.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback(fn: (e: any) => void) {
          callbacks.set(++cb, fn);
          return cb;
        },
        async invoke(command: string, args: any) {
          if (command === "plugin:event|listen") {
            if (args.event === "engine-event") listeners.add(args.handler);
            return args.handler;
          }
          if (command === "plugin:event|unlisten") {
            listeners.delete(args.eventId);
            return;
          }
          if (command === "plugin:window|hide") {
            state.__hiddenNotes = { ...state.__saved };
            return;
          }
          if (command !== "engine_call") return;
          const p = args.params || {};
          state.__calls.push({
            method: args.method,
            params: structuredClone(p),
          });
          switch (args.method) {
            case "bootstrap":
              throw new Error("Eager bootstrap is forbidden in the native UI.");
            case "bootstrap_library":
              return {
                meetingPage: meetingPage({ limit: p.limit }),
                library: overview(),
                settings,
                status,
                accounts: [],
              };
            case "library_overview":
              return overview();
            case "list_meetings":
              return meetingPage(p);
            case "get_meeting":
              if (state.__failDetails[p.meetingId])
                throw new Error("This conversation could not be opened.");
              if (state.__gates[p.meetingId])
                await new Promise<void>((resolve) => {
                  state.__releases[p.meetingId] = () => {
                    delete state.__gates[p.meetingId];
                    resolve();
                  };
                });
              return full(p.meetingId);
            case "get_status":
              return status;
            case "get_settings":
              return settings;
            case "list_calendar_events":
              return [];
            case "meeting_audio":
              return [];
            case "note_history":
              return [];
            case "list_task_page": {
              const all = scoped(p)
                  .filter((m) => m.taskCount)
                  .map((m) => ({
                    ...full(m.id).tasks[0],
                    meetingId: m.id,
                    meetingTitle: m.title,
                  }))
                  .filter((t) => p.includeDone || !t.done),
                offset = Number(p.cursor || 0);
              return {
                tasks: all.slice(offset, offset + 50),
                total: all.length,
                nextCursor:
                  offset + 50 < all.length ? String(offset + 50) : null,
              };
            }
            case "update_task":
              if (state.__holdTask)
                await new Promise<void>((resolve) => {
                  state.__releaseTask = resolve;
                });
              state.__saved[`done-${Number(p.meetingId.split("-")[1])}`] =
                p.changes.done;
              return full(p.meetingId);
            case "update_meeting":
              if (p.changes.notes !== undefined)
                state.__saved[p.id] = p.changes.notes;
              return { ...full(p.id), ...p.changes };
            case "search_library":
              return [
                {
                  meetingId: "meeting-119",
                  meetingTitle: "Conversation 119",
                  source: "transcript",
                  segmentId: "segment-119-0",
                  start: 0,
                  text: "obscure marker",
                  url: "tuck://meeting/meeting-119?source=transcript&segment=segment-119-0&start=0",
                },
              ];
            default:
              throw new Error(`Missing fixture method ${args.method}`);
          }
        },
      };
    },
    { base: exampleMeeting(), settings: defaults, options },
  );
}
