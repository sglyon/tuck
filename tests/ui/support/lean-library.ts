import type { Page } from "@playwright/test";

/** Adapt older, stateful native test ports to the lean API. Production never uses this adapter. */
export async function installLeanLibraryPort(page: Page) {
  await page.addInitScript(() => {
    const state = window as unknown as Record<string, any>;
    const wrap = (port: any) => {
      if (!port || port.__leanFixture) return port;
      const invoke = port.invoke.bind(port);
      port.__leanFixture = true;
      port.invoke = async (command: string, args: Record<string, any>) => {
        if (
          command !== "engine_call" ||
          ![
            "bootstrap_library",
            "list_meetings",
            "library_overview",
            "get_meeting",
            "list_task_page",
          ].includes(args.method)
        )
          return invoke(command, args);
        const data = await invoke("engine_call", {
          method: "bootstrap",
          params: {},
        });
        const meetings = data.meetings;
        const p = args.params || {};
        const header = (m: any) => {
          const {
            notes,
            summary,
            segments,
            speakers,
            messages,
            bookmarks,
            tasks,
            ...metadata
          } = m;
          return {
            ...metadata,
            segmentCount: segments.length,
            speakerCount: speakers.length,
            messageCount: messages.length,
            bookmarkCount: bookmarks.length,
            taskCount: (tasks || []).length,
            openTaskCount: (tasks || []).filter((t: any) => !t.done).length,
            hasNotes: !!notes,
            hasSummary: !!summary,
          };
        };
        const values = (key: string) =>
          [
            ...new Set(
              meetings.flatMap((m: any) =>
                key === "folders" ? (m.folder ? [m.folder] : []) : m[key] || [],
              ),
            ),
          ].sort();
        const overview = {
          meetingCount: meetings.length,
          starredCount: meetings.filter((m: any) => m.starred).length,
          openTaskCount: meetings
            .flatMap((m: any) => m.tasks || [])
            .filter((t: any) => !t.done).length,
          folders: values("folders"),
          tags: values("tags"),
          people: values("people"),
          facetsTruncated: { folders: false, tags: false, people: false },
          activeMeetingId: data.status.activeMeetingId,
        };
        const scope = p.scope || {};
        const query = String(p.query || "").toLowerCase();
        const scoped = meetings.filter(
          (m: any) =>
            (!scope.folder || m.folder === scope.folder) &&
            (!scope.tags?.length ||
              scope.tags.every((t: string) => m.tags?.includes(t))) &&
            (!scope.people?.length ||
              scope.people.every((t: string) => m.people?.includes(t))) &&
            (p.starred === undefined || m.starred === p.starred) &&
            (!query ||
              [
                m.title,
                m.notes,
                m.summary,
                ...m.segments.map((s: any) => s.text),
              ].some((text: string) => text.toLowerCase().includes(query))),
        );
        const tasks = args.method === "list_task_page";
        const items = tasks
          ? scoped.flatMap((m: any) =>
              (m.tasks || [])
                .filter((t: any) => p.includeDone || !t.done)
                .map((t: any) => ({
                  ...t,
                  meetingId: m.id,
                  meetingTitle: m.title,
                })),
            )
          : scoped.map(header);
        const offset = Number(p.cursor || 0),
          limit = p.limit || 50;
        const meetingPage = {
          meetings: items.slice(offset, offset + limit),
          total: items.length,
          nextCursor:
            offset + limit < items.length ? String(offset + limit) : null,
        };
        if (args.method === "bootstrap_library")
          return {
            ...data,
            meetings: undefined,
            meetingPage,
            library: overview,
          };
        if (args.method === "library_overview") return overview;
        if (args.method === "get_meeting") {
          const found = meetings.find((m: any) => m.id === p.meetingId);
          if (!found) throw new Error("Conversation not found.");
          return found;
        }
        return tasks
          ? {
              tasks: meetingPage.meetings,
              total: meetingPage.total,
              nextCursor: meetingPage.nextCursor,
            }
          : meetingPage;
      };
      return port;
    };
    let port = wrap(state.__TAURI_INTERNALS__);
    Object.defineProperty(state, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => port,
      set: (value) => {
        port = wrap(value);
      },
    });
  });
}
