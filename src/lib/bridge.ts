import type {
  Bootstrap,
  EngineEvent,
  Meeting,
  Settings,
  EngineStatus,
  MeetingChanges,
  ExportFormat,
} from "./types";
import { meetingHeader } from "./library";
import { exampleMeeting } from "./example";
import { exportContent } from "./format";
export const native =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const defaults: Settings = {
  provider: "local",
  model: "qwen3:4b",
  localUrl: "http://127.0.0.1:11434",
  hostedConsent: false,
  webSearch: false,
  autoSummary: false,
  notionAutoSync: false,
  notionTarget: "",
  notionPropertyMapping: {},
  threads: 2,
  keepAudio: true,
  hasOpenAIKey: false,
  hasNotionToken: false,
  customTemplates: [],
  recipes: [],
  microphoneDeviceId: "",
  systemDeviceId: "",
};
const previewStatus: EngineStatus = {
  engineVersion: "preview",
  platform: "browser",
  modelInstalled: false,
  modelDownloading: false,
  modelProgress: 0,
  speakerModelInstalled: false,
  activeMeetingId: null,
  recordingState: "idle",
  captureAvailable: false,
};
const key = "tuck-preview-v1";
let data: { meetings: Meeting[]; settings: Settings } = {
  meetings: [],
  settings: defaults,
};
if (!native) {
  try {
    const raw = localStorage.getItem(key);
    if (raw) {
      const stored = JSON.parse(raw);
      if (Array.isArray(stored.meetings))
        data = {
          meetings: stored.meetings,
          settings: {
            ...defaults,
            ...stored.settings,
            hasOpenAIKey: false,
            hasNotionToken: false,
          },
        };
    }
  } catch {
    /* A blocked browser store still supports a session preview. */
  }
}
let previewGeneration = 0;
function persist() {
  previewGeneration++;
  try {
    localStorage.setItem(key, JSON.stringify(data));
  } catch {
    /* Keep the session usable when storage is unavailable. */
  }
}
function previewCall(method: string, p: Record<string, unknown>): unknown {
  const get = (id = p.meetingId ?? p.id) => {
    const m = data.meetings.find((m) => m.id === id);
    if (!m)
      throw new Error(
        "This conversation is no longer available. Choose another from the library.",
      );
    return m;
  };
  switch (method) {
    case "bootstrap":
      return { ...structuredClone(data), status: previewStatus, accounts: [] };
    case "bootstrap_library":
      return {
        meetingPage: previewCall("list_meetings", { limit: p.limit ?? 50 }),
        library: previewCall("library_overview", {}),
        settings: structuredClone(data.settings),
        status: previewStatus,
        accounts: [],
      };
    case "get_meeting":
      return structuredClone(get());
    case "library_overview": {
      const values = (field: "folder" | "tags" | "people") =>
        [
          ...new Set(
            data.meetings.flatMap((m) =>
              field === "folder"
                ? m.folder
                  ? [m.folder]
                  : []
                : m[field] || [],
            ),
          ),
        ].sort();
      const folders = values("folder"),
        tags = values("tags"),
        people = values("people");
      return {
        meetingCount: data.meetings.length,
        starredCount: data.meetings.filter((m) => m.starred).length,
        openTaskCount: data.meetings
          .flatMap((m) => m.tasks || [])
          .filter((t) => !t.done).length,
        folders: folders.slice(0, 200),
        tags: tags.slice(0, 200),
        people: people.slice(0, 200),
        facetsTruncated: {
          folders: folders.length > 200,
          tags: tags.length > 200,
          people: people.length > 200,
        },
        activeMeetingId: null,
      };
    }
    case "list_meetings":
    case "list_task_page": {
      const { cursor } = p;
      const scope = (p.scope || {}) as {
        folder?: string;
        tags?: string[];
        people?: string[];
        meetingIds?: string[];
      };
      const filterKey = JSON.stringify({
        method,
        query: String(p.query || "").trim(),
        starred: p.starred ?? null,
        includeDone: !!p.includeDone,
        scope: {
          folder: scope.folder || undefined,
          tags: [...new Set(scope.tags || [])].sort(),
          people: [...new Set(scope.people || [])].sort(),
          meetingIds: [...new Set(scope.meetingIds || [])].sort(),
        },
      });
      let offset = 0;
      if (cursor) {
        let value;
        try {
          value = JSON.parse(String(cursor));
        } catch {
          throw new Error(
            "This library page could not be read. Try loading it again.",
          );
        }
        if (
          value.generation !== previewGeneration ||
          value.filter !== filterKey
        )
          throw new EngineCallError(
            "Your library changed. Loading its latest page.",
            "cursor_stale",
          );
        offset = value.offset;
      }
      const query = String(p.query || "")
        .trim()
        .replace(/[A-Z]/g, (c) => c.toLowerCase());
      const matches = data.meetings
        .filter(
          (m) =>
            (!scope.folder || m.folder === scope.folder) &&
            (!scope.tags?.length ||
              scope.tags.every((t) => m.tags?.includes(t))) &&
            (!scope.people?.length ||
              scope.people.every((t) => m.people?.includes(t))) &&
            (!scope.meetingIds?.length || scope.meetingIds.includes(m.id)) &&
            (p.starred === undefined || m.starred === p.starred) &&
            (!query ||
              [
                m.title,
                m.notes,
                m.summary,
                ...m.segments.map((s) => s.text),
              ].some((text) =>
                text.replace(/[A-Z]/g, (c) => c.toLowerCase()).includes(query),
              )),
        )
        .sort(
          (a, b) =>
            b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id),
        );
      const tasks = method === "list_task_page";
      const items = tasks
        ? matches
            .flatMap((m) =>
              (m.tasks || [])
                .filter((t) => p.includeDone || !t.done)
                .map((t) => ({ ...t, meetingId: m.id, meetingTitle: m.title })),
            )
            .sort(
              (a, b) =>
                Number(a.done) - Number(b.done) ||
                (a.dueDate || "9999").localeCompare(b.dueDate || "9999") ||
                a.createdAt.localeCompare(b.createdAt) ||
                a.meetingId.localeCompare(b.meetingId) ||
                a.id.localeCompare(b.id),
            )
        : matches.map(meetingHeader);
      const limit = Math.min(100, Math.max(1, Number(p.limit || 50)));
      return {
        [tasks ? "tasks" : "meetings"]: structuredClone(
          items.slice(offset, offset + limit),
        ),
        total: items.length,
        nextCursor:
          offset + limit < items.length
            ? JSON.stringify({
                generation: previewGeneration,
                filter: filterKey,
                offset: offset + limit,
              })
            : null,
      };
    }
    case "get_status":
      return previewStatus;
    case "get_settings":
      return data.settings;
    case "create_meeting": {
      const m: Meeting = {
        id: crypto.randomUUID(),
        title: String(p.title || "Untitled conversation"),
        createdAt: new Date().toISOString(),
        duration: 0,
        status: "draft",
        notes: "",
        summary: "",
        template: "general",
        starred: false,
        isExample: false,
        notionUrl: null,
        notionStatus: "idle",
        segments: [],
        speakers: [],
        messages: [],
        bookmarks: [],
      };
      data.meetings.unshift(m);
      persist();
      return structuredClone(m);
    }
    case "load_example": {
      const previous = data.meetings.find((m) => m.isExample);
      if (previous) return structuredClone(previous);
      const m = exampleMeeting();
      data.meetings.unshift(m);
      persist();
      return structuredClone(m);
    }
    case "update_meeting": {
      const m = get();
      const c = p.changes as MeetingChanges;
      Object.assign(m, c);
      persist();
      return structuredClone(m);
    }
    case "delete_meeting":
      data.meetings = data.meetings.filter((m) => m.id !== p.id);
      persist();
      return null;
    case "meeting_audio":
      return [];
    case "correct_transcript":
    case "reset_transcript": {
      const m = get();
      if (["recording", "paused", "processing"].includes(m.status))
        throw new Error("Finish recording before correcting the transcript.");
      const segment = m.segments.find((s) => s.id === p.segmentId);
      if (!segment)
        throw new Error("This transcript passage is no longer available.");
      if (segment.text !== p.baseText)
        throw new Error(
          "This passage changed. Reopen the correction to use its latest text.",
        );
      const original = segment.originalText ?? segment.text;
      const text =
        method === "reset_transcript" ? original : String(p.text || "").trim();
      if (!text || text.length > 32000)
        throw new Error(
          "Use between 1 and 32,000 characters for a correction.",
        );
      segment.originalText = original;
      segment.text = text;
      segment.edited = text !== original;
      if (segment.edited) segment.correctedAt = new Date().toISOString();
      else delete segment.correctedAt;
      persist();
      return structuredClone(m);
    }
    case "rename_speaker": {
      const m = get();
      const s = m.speakers.find((s) => s.id === p.speakerId);
      if (!s) throw new Error("Choose a speaker from this transcript.");
      s.name = String(p.name).trim() || s.name;
      persist();
      return structuredClone(m);
    }
    case "add_bookmark": {
      const m = get();
      m.bookmarks.push({
        id: crypto.randomUUID(),
        time: Number(p.time),
        label: String(p.label || "Marked moment"),
      });
      persist();
      return structuredClone(m);
    }
    case "remove_bookmark": {
      const m = get();
      m.bookmarks = m.bookmarks.filter((b) => b.id !== p.id);
      persist();
      return structuredClone(m);
    }
    case "update_settings":
      data.settings = { ...data.settings, ...(p.changes as Partial<Settings>) };
      persist();
      return structuredClone(data.settings);
    case "upsert_template":
    case "upsert_recipe": {
      const templates = method === "upsert_template";
      const item = p[templates ? "template" : "recipe"] as {
        id?: string;
        name: string;
        instructions?: string;
        prompt?: string;
      };
      if (!item.name.trim() || !(item.instructions || item.prompt || "").trim())
        throw new Error("Add a name and instructions first.");
      const field = templates ? "customTemplates" : "recipes";
      const saved = { ...item, id: item.id || crypto.randomUUID() };
      const items = data.settings[field] || [];
      (data.settings[field] as unknown[]) = [
        ...items.filter((i) => i.id !== saved.id),
        saved,
      ];
      persist();
      return structuredClone(data.settings[field]);
    }
    case "delete_template":
    case "delete_recipe": {
      const field =
        method === "delete_template" ? "customTemplates" : "recipes";
      data.settings[field] = data.settings[field].filter(
        (i) => i.id !== p.id,
      ) as never;
      persist();
      return structuredClone(data.settings[field]);
    }
    case "add_task": {
      const m = get();
      if (!String(p.title || "").trim())
        throw new Error("Add a task title first.");
      (m.tasks ||= []).push({
        id: crypto.randomUUID(),
        title: String(p.title).trim(),
        owner: String(p.owner || ""),
        dueDate: String(p.dueDate || ""),
        done: false,
        createdAt: new Date().toISOString(),
      });
      persist();
      return structuredClone(m);
    }
    case "update_task":
    case "delete_task": {
      const m = get();
      m.tasks = (m.tasks || [])
        .filter((t) => method !== "delete_task" || t.id !== p.id)
        .map((t) => (t.id === p.id ? { ...t, ...(p.changes as object) } : t));
      persist();
      return structuredClone(m);
    }
    case "list_tasks":
      return data.meetings.flatMap((m) =>
        (m.tasks || [])
          .filter((t) => p.includeDone || !t.done)
          .map((t) => ({ ...t, meetingId: m.id, meetingTitle: m.title })),
      );
    case "search_library": {
      const q = String(p.query || "")
        .toLowerCase()
        .trim();
      if (!q) return [];
      return data.meetings
        .filter(
          (m) =>
            (!p.folder || m.folder === p.folder) &&
            (!(p.tags as string[] | undefined)?.length ||
              (p.tags as string[]).every((t) => m.tags?.includes(t))) &&
            (!(p.people as string[] | undefined)?.length ||
              (p.people as string[]).every((t) => m.people?.includes(t))),
        )
        .flatMap((m) => [
          ...m.segments.map((segment) => ({
            meetingId: m.id,
            meetingTitle: m.title,
            segmentId: segment.id,
            start: segment.start,
            text: segment.text,
            source: "transcript",
            url: `tuck://meeting/${m.id}?segment=${segment.id}&start=${segment.start}`,
          })),
          ...["notes", "summary", "title"].map((source) => ({
            meetingId: m.id,
            meetingTitle: m.title,
            text: m[source as "notes" | "summary" | "title"],
            source,
            url: `tuck://meeting/${m.id}?source=${source}`,
          })),
        ])
        .filter((h) => h.text.toLowerCase().includes(q))
        .slice(0, Number(p.limit || 24));
    }
    case "note_history":
      return [];
    case "chatgpt_accounts":
      return [];
    case "export_meeting":
      return { content: exportContent(get(), p.format as ExportFormat) };
    default:
      throw new Error(
        ["chat", "summarize", "chat_library", "propose_notes"].includes(method)
          ? "AI runs in the desktop app. Open Tuck on your computer and connect a local model or your ChatGPT account in Settings."
          : method === "start_recording"
            ? "Recording requires the desktop app and microphone permission. This browser preview never captures audio."
            : "This connection is available in the desktop app. Open Tuck on your computer to set it up.",
      );
  }
}
export class EngineCallError extends Error {
  constructor(
    message: string,
    public code?: string,
  ) {
    super(message);
    this.name = "EngineCallError";
  }
}
export async function call<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  if (native) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke<T>("engine_call", { method, params });
    } catch (err) {
      if (typeof err === "string") {
        try {
          const value = JSON.parse(err);
          throw new EngineCallError(
            value.message ?? value.error?.message ?? err,
            value.code ?? value.error?.code,
          );
        } catch (parsed) {
          if (parsed instanceof SyntaxError) throw new Error(err);
          throw parsed;
        }
      }
      if (err && typeof err === "object" && "message" in err)
        throw new EngineCallError(
          String(err.message),
          "code" in err ? String(err.code) : undefined,
        );
      throw new Error(
        "The local engine did not respond. Your saved meetings are safe; restart Tuck and try again.",
      );
    }
  }
  return previewCall(method, params) as T;
}
export async function subscribe(
  handler: (event: EngineEvent) => void,
): Promise<() => void> {
  if (!native) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<EngineEvent>("engine-event", (e) => handler(e.payload));
}
export async function openExternal(url: string) {
  if (!/^https?:\/\//.test(url)) return;
  if (native) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else window.open(url, "_blank", "noopener,noreferrer");
}
export async function selectAudio(): Promise<string | null> {
  if (!native)
    throw new Error(
      "Import audio in the desktop app. Tuck prepares and transcribes supported audio files locally.",
    );
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    multiple: false,
    filters: [
      {
        name: "Supported audio",
        extensions: ["wav", "mp3", "flac", "m4a", "mp4", "aac", "adts"],
      },
    ],
  });
  return typeof result === "string" ? result : null;
}
export async function saveExport(meeting: Meeting, format: ExportFormat) {
  const ext = { markdown: "md", json: "json", srt: "srt" }[format];
  const name = `${meeting.title.replace(/[^a-zA-Z0-9 _-]/g, "").trim() || "meeting"}.${ext}`;
  if (native) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: name,
      filters: [
        {
          name: format === "markdown" ? "Markdown" : format.toUpperCase(),
          extensions: [ext],
        },
      ],
    });
    if (!path) return false;
    await call("export_meeting", { meetingId: meeting.id, format, path });
  } else {
    const { content } = await call<{ content: string }>("export_meeting", {
      meetingId: meeting.id,
      format,
    });
    const url = URL.createObjectURL(
      new Blob([content], { type: "text/plain;charset=utf-8" }),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
  }
  return true;
}

export async function chooseBackupFile(): Promise<string | null> {
  if (!native)
    throw new Error("Backup and restore use local files in the desktop app.");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    multiple: false,
    filters: [{ name: "Tuck library backup", extensions: ["zip"] }],
  });
  return typeof result === "string" ? result : null;
}
export async function chooseBackupDestination(): Promise<string | null> {
  if (!native)
    throw new Error("Backup and restore use local files in the desktop app.");
  const { save } = await import("@tauri-apps/plugin-dialog");
  return await save({
    defaultPath: `Tuck library ${new Date().toISOString().slice(0, 10)}.zip`,
    filters: [{ name: "Tuck library backup", extensions: ["zip"] }],
  });
}
