import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type SetStateAction,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowLeft,
  ArrowRight,
  AudioLines,
  Bookmark,
  BookOpen,
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  Download,
  FileAudio,
  FileText,
  Folder,
  Headphones,
  Library,
  LoaderCircle,
  Menu,
  MessageSquare,
  Mic,
  Monitor,
  MoreHorizontal,
  Pause,
  Play,
  Plus,
  Search,
  Send,
  Settings2,
  ShieldCheck,
  Sparkles,
  Square,
  Star,
  Trash2,
  Upload,
  Users,
  X,
} from "lucide-react";
import { useLibraryIndex } from "./lib/useLibraryIndex";
import { meetingHeader, pruneDetails } from "./lib/library";
import Settings from "./components/Settings";
import {
  RetainedAudio,
  TranscriptCorrection,
} from "./components/TranscriptTools";
import LibraryDesk, {
  ScopeFilters,
  type LibraryScope,
} from "./components/LibraryDesk";
import { Organization, Tasks, WritingTools } from "./components/MeetingTools";
import UpcomingMeetings from "./components/UpcomingMeetings";
import { NotionRecovery } from "./components/NotionDestination";
import { Markdown } from "./components/Markdown";
import {
  call,
  defaults,
  native,
  openExternal,
  saveExport,
  selectAudio,
  subscribe,
} from "./lib/bridge";
import {
  clock,
  duration,
  fullDate,
  initials,
  meetingDate,
  relativeGroup,
} from "./lib/format";
import type {
  Account,
  LibraryBootstrap,
  EngineEvent,
  EngineStatus,
  ExportFormat,
  Meeting,
  MeetingChanges,
  Message,
  Segment,
  Settings as SettingsData,
  Speaker,
  TranscriptDraft,
} from "./lib/types";

type Tab = "summary" | "notes" | "transcript" | "tasks";
type View = "meeting" | "settings" | "library" | "tasks";
type MobilePane = "library" | "document" | "assistant";
const emptyStatus: EngineStatus = {
  engineVersion: "…",
  platform: "desktop",
  modelInstalled: false,
  modelDownloading: false,
  modelProgress: 0,
  speakerModelInstalled: false,
  activeMeetingId: null,
  recordingState: "idle",
  captureAvailable: false,
};
function Brand() {
  return (
    <div className="brand">
      <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true">
        <rect width="28" height="28" rx="8" fill="currentColor" />
        <path
          d="M8 8v8a5 5 0 0 0 5 5h2M13 5v11a1 1 0 0 0 1 1h6M18 9v4h4"
          stroke="white"
          strokeWidth="2.3"
          fill="none"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <span>
        tuck<span className="brand-dot">.</span>
      </span>
    </div>
  );
}
function Avatar({
  speaker,
  small = false,
}: {
  speaker: Speaker;
  small?: boolean;
}) {
  return (
    <span
      className={`avatar speaker-${speaker.color % 6}${small ? " small" : ""}`}
      aria-hidden="true"
    >
      {initials(speaker.name)}
    </span>
  );
}
function MarkedText({ text, query }: { text: string; query: string }) {
  if (!query.trim()) return <>{text}</>;
  const at = text.toLowerCase().indexOf(query.toLowerCase());
  if (at < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, at)}
      <mark>{text.slice(at, at + query.length)}</mark>
      {text.slice(at + query.length)}
    </>
  );
}
function EmptyTranscript({ recording }: { recording: boolean }) {
  return (
    <div className="empty-inner">
      <AudioLines size={29} strokeWidth={1.4} />
      <h3>
        {recording ? "Listening, locally." : "The conversation starts here."}
      </h3>
      <p>
        {recording
          ? "Spoken sentences will appear here as they are transcribed."
          : "Start recording or import an audio file. Tuck keeps both sides of the conversation on your computer."}
      </p>
      {!recording && (
        <p className="quiet-note">
          Import WAV, MP3, FLAC, or AAC-LC audio in M4A, MP4, AAC and ADTS
          files. Protected tracks and other codecs are not supported.
        </p>
      )}
    </div>
  );
}
function ConfirmDialog({
  open,
  title,
  description,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: string;
  description: string;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (open && !ref.current?.open) ref.current?.showModal();
    else if (!open && ref.current?.open) ref.current.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      className="confirm-dialog"
      onCancel={onClose}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div>
        <div className="dialog-heading">
          <h2>{title}</h2>
          <button
            className="icon-button"
            aria-label="Close confirmation"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <p>{description}</p>
        <div className="dialog-actions">
          <button className="button secondary" onClick={onClose} autoFocus>
            Keep conversation
          </button>
          <button className="button danger" onClick={onConfirm}>
            <Trash2 size={15} />
            Delete conversation
          </button>
        </div>
      </div>
    </dialog>
  );
}

export default function App() {
  const [meetings, commitMeetings] = useState<Meeting[]>([]);
  const [settings, setSettings] = useState<SettingsData>(defaults);
  const [status, setStatus] = useState<EngineStatus>(emptyStatus);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [view, setView] = useState<View>("meeting");
  const [settingsSection, setSettingsSection] = useState("Intelligence");
  const [tab, setTab] = useState<Tab>("summary");
  const [pane, setPane] = useState<MobilePane>("document");
  const [scope, setScope] = useState<LibraryScope>({
    folder: "",
    tag: "",
    person: "",
  });
  const [editingRecap, setEditingRecap] = useState(false);
  const [recapStates, setRecapStates] = useState<
    Record<string, "saving" | "saved" | "error">
  >({});
  const pendingRecaps = useRef(new Map<string, string>());
  const recapSaves = useRef(new Map<string, Promise<boolean>>());
  const recapTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const desktopClosing = useRef(false);
  const quickCapturePending = useRef(false);
  const desktopIntent = useRef<(intent: string) => Promise<void>>(
    async () => {},
  );
  const [loading, setLoading] = useState(true);
  const [bootError, setBootError] = useState("");
  const [correctionDrafts, setCorrectionDrafts] = useState<
    Record<string, TranscriptDraft>
  >({});
  const pendingCorrections = useRef(new Map<string, TranscriptDraft>());
  const audioSeek = useRef<((seconds: number) => void) | null>(null);
  const bindAudioSeek = useCallback(
    (seek: ((seconds: number) => void) | null) => {
      audioSeek.current = seek;
    },
    [],
  );
  const [recoverRequested, setRecoverRequested] = useState<string | null>(null);
  const changeCorrection = (
    meetingId: string,
    segmentId: string,
    draft: TranscriptDraft | null,
  ) => {
    const key = JSON.stringify([meetingId, segmentId]);
    if (draft) pendingCorrections.current.set(key, draft);
    else pendingCorrections.current.delete(key);
    setCorrectionDrafts((prev) => {
      const next = { ...prev };
      if (draft) next[key] = draft;
      else delete next[key];
      return next;
    });
  };
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "starred">("all");
  const library = useLibraryIndex(query, filter === "starred", scope);
  const [detailLoading, setDetailLoading] = useState<string | null>(null);
  const [detailError, setDetailError] = useState<{
    id: string;
    text: string;
  } | null>(null);
  const details = useRef<Meeting[]>([]);
  const detailVersions = useRef(new Map<string, number>());
  const detailUse = useRef(new Map<string, number>());
  const detailClock = useRef(0);
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;
  const detailRequests = useRef(new Map<string, Promise<Meeting>>());
  const operationOwners = useRef(new Set<string>());
  const detailPins = useRef<() => Set<string>>(() => new Set());
  const setMeetings = useCallback((update: SetStateAction<Meeting[]>) => {
    const next =
      typeof update === "function" ? update(details.current) : update;
    details.current = pruneDetails(
      next,
      detailPins.current(),
      detailUse.current,
    );
    commitMeetings(details.current);
  }, []);
  const bumpDetail = (id: string) =>
    detailVersions.current.set(id, (detailVersions.current.get(id) || 0) + 1);
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; error: boolean } | null>(
    null,
  );
  const [assistantOpen, setAssistantOpen] = useState(true);
  const [microphone, setMicrophone] = useState(true);
  const [systemAudio, setSystemAudio] = useState(true);
  const [transcriptQuery, setTranscriptQuery] = useState("");
  const [highlight, setHighlight] = useState<string | null>(null);
  const [followLive, setFollowLive] = useState(true);
  const [showExport, setShowExport] = useState(false);
  const [showMore, setShowMore] = useState(false);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [renameSpeaker, setRenameSpeaker] = useState<Speaker | null>(null);
  const [speakerName, setSpeakerName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [stream, setStream] = useState<{
    meetingId: string;
    requestId: string;
    text: string;
  } | null>(null);
  const [chatBusy, setChatBusy] = useState<string | null>(null);
  const [clockValues, setClockValues] = useState<Record<string, number>>({});
  const [noteStates, setNoteStates] = useState<
    Record<string, "saving" | "saved" | "error">
  >({});
  const pendingNotes = useRef(new Map<string, string>());
  const noteSaves = useRef(new Map<string, Promise<boolean>>());
  const noteTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const transcriptEnd = useRef<HTMLDivElement>(null);
  const messageEnd = useRef<HTMLDivElement>(null);
  const chatRequest = useRef<string | null>(null);
  detailPins.current = () =>
    new Set(
      [
        selectedRef.current,
        status.activeMeetingId,
        ...pendingNotes.current.keys(),
        ...pendingRecaps.current.keys(),
        ...[...pendingCorrections.current.values()].map((d) => d.meetingId),
        ...operationOwners.current,
        chatBusy,
      ].filter((id): id is string => !!id),
    );
  const meeting = meetings.find((m) => m.id === selectedId) ?? null;
  const capture = meetings.find(
    (m) =>
      m.status === "recording" ||
      m.status === "paused" ||
      m.status === "processing",
  );
  const active =
    meeting?.status === "recording" || meeting?.status === "paused";
  const recording = meeting?.status === "recording";
  const elapsed = meeting ? (clockValues[meeting.id] ?? meeting.duration) : 0;
  const notify = useCallback((text: string, error = false) => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
    setToast({ text, error });
    if (!error) toastTimer.current = setTimeout(() => setToast(null), 5000);
  }, []);
  const merge = useCallback(
    (m: Meeting, mutated = true) => {
      bumpDetail(m.id);
      detailUse.current.set(m.id, ++detailClock.current);
      library.upsert(meetingHeader(m), mutated);
      setMeetings((prev) => {
        const local = pendingNotes.current.get(m.id);
        const next = {
          ...m,
          ...(local === undefined ? {} : { notes: local }),
          ...(pendingRecaps.current.has(m.id)
            ? { summary: pendingRecaps.current.get(m.id)! }
            : {}),
        };
        return prev.some((p) => p.id === m.id)
          ? prev.map((p) => (p.id === m.id ? next : p))
          : [next, ...prev];
      });
    },
    [library.upsert, setMeetings],
  );
  const refreshStatus = useCallback(
    async () => setStatus(await call<EngineStatus>("get_status")),
    [],
  );
  const bootstrap = useCallback(async () => {
    setLoading(true);
    setBootError("");
    try {
      const data = await call<LibraryBootstrap>("bootstrap_library", {
        limit: 50,
      });
      library.install(data);
      setMeetings([]);
      setSettings({ ...defaults, ...data.settings });
      setStatus(data.status);
      setAccounts(data.accounts);
      setSelectedId(
        (prev) =>
          prev ||
          data.library.activeMeetingId ||
          data.meetingPage.meetings[0]?.id ||
          null,
      );
    } catch (e) {
      setBootError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [library.install, setMeetings]);
  const loadMeeting = useCallback(
    async (id: string, force = false): Promise<Meeting> => {
      const cached = details.current.find((m) => m.id === id);
      detailUse.current.set(id, ++detailClock.current);
      if (cached && !force) return cached;
      const pending = detailRequests.current.get(id);
      if (pending) return pending;
      const request = (async () => {
        for (let attempt = 0; attempt < 2; attempt++) {
          const version = detailVersions.current.get(id) || 0;
          const m = await call<Meeting>("get_meeting", { meetingId: id });
          if (version === (detailVersions.current.get(id) || 0)) {
            merge(m, false);
            return details.current.find((p) => p.id === id) || m;
          }
          const current = details.current.find((p) => p.id === id);
          if (current) return current;
        }
        throw new Error("This conversation changed while opening. Try again.");
      })();
      detailRequests.current.set(id, request);
      try {
        return await request;
      } finally {
        if (detailRequests.current.get(id) === request)
          detailRequests.current.delete(id);
      }
    },
    [merge],
  );
  useEffect(() => {
    if (loading || view !== "meeting" || !selectedId) return;
    let current = true;
    setDetailLoading(selectedId);
    setDetailError(null);
    void loadMeeting(selectedId)
      .catch((e) => {
        if (current)
          setDetailError({
            id: selectedId,
            text: e instanceof Error ? e.message : String(e),
          });
      })
      .finally(() => {
        if (current) setDetailLoading(null);
      });
    return () => {
      current = false;
    };
  }, [selectedId, view, loading, loadMeeting]);
  useEffect(() => {
    if (
      !loading &&
      status.activeMeetingId &&
      status.activeMeetingId !== selectedId
    )
      void loadMeeting(status.activeMeetingId).catch(() => {});
  }, [loading, status.activeMeetingId, selectedId, loadMeeting]);
  useEffect(() => {
    void bootstrap();
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void subscribe((e: EngineEvent) => {
      const p = e.payload;
      switch (e.event) {
        case "desktop_intent":
          void desktopIntent
            .current(String(p.intent))
            .catch((e) =>
              notify(e instanceof Error ? e.message : String(e), true),
            );
          break;
        case "meeting_updated": {
          const incoming = p.meeting as Meeting;
          const previous = details.current.find((m) => m.id === incoming.id);
          const speechOnly =
            previous &&
            ["recording", "paused", "processing"].includes(incoming.status) &&
            JSON.stringify({
              ...meetingHeader(previous),
              duration: 0,
              segmentCount: 0,
              status: "",
            }) ===
              JSON.stringify({
                ...meetingHeader(incoming),
                duration: 0,
                segmentCount: 0,
                status: "",
              }) &&
            previous.notes === incoming.notes &&
            previous.summary === incoming.summary &&
            JSON.stringify(previous.tasks) === JSON.stringify(incoming.tasks);
          merge(incoming, !speechOnly);
          if (speechOnly) library.refresh(false);
          break;
        }
        case "segment":
          bumpDetail(String(p.meetingId));
          library.refresh(false);
          setMeetings((prev) =>
            prev.map((m) =>
              m.id === p.meetingId
                ? {
                    ...m,
                    segments: m.segments.some(
                      (s) => s.id === (p.segment as Segment).id,
                    )
                      ? m.segments
                      : [...m.segments, p.segment as Segment],
                  }
                : m,
            ),
          );
          break;
        case "recording_state":
          library.patch(String(p.meetingId), {
            status: p.state as Meeting["status"],
          });
          bumpDetail(String(p.meetingId));
          library.refresh(false);
          if (
            [
              "draft",
              "recording",
              "paused",
              "processing",
              "complete",
              "interrupted",
            ].includes(String(p.state))
          )
            setMeetings((prev) =>
              prev.map((m) =>
                m.id === p.meetingId
                  ? { ...m, status: p.state as Meeting["status"] }
                  : m,
              ),
            );
          setStatus((s) => ({
            ...s,
            activeMeetingId: ["recording", "paused", "processing"].includes(
              p.state as string,
            )
              ? (p.meetingId as string)
              : null,
            recordingState: p.state as string,
          }));
          break;
        case "model_progress": {
          const stage = String(p.stage).toLowerCase();
          const done = ["complete", "error", "installed", "failed"].includes(
            stage,
          );
          setStatus((s) => ({
            ...s,
            modelDownloading: !done,
            modelProgress: Number(p.progress) || 0,
            ...(stage === "installed" && p.kind === "parakeet"
              ? { modelInstalled: true }
              : {}),
            ...(stage === "installed" && p.kind === "speakers"
              ? { speakerModelInstalled: true }
              : {}),
          }));
          break;
        }
        case "chat_delta":
          if (chatRequest.current === p.requestId)
            setStream((prev) =>
              prev && prev.requestId === p.requestId
                ? { ...prev, text: prev.text + String(p.delta) }
                : prev,
            );
          break;
        case "recording_warning":
          notify(
            String(
              p.message ||
                "No sound detected. Check your audio sources; recording continues.",
            ),
          );
          break;
        case "engine_error":
          notify(
            String(
              p.message ||
                "The local engine needs attention. Open Settings to check your configuration.",
            ),
            true,
          );
          break;
      }
    }).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [bootstrap, merge, notify, library.refresh, setMeetings]);
  useEffect(() => {
    if (!capture || capture.status !== "recording") return;
    const id = capture.id;
    setClockValues((v) => ({ ...v, [id]: v[id] ?? capture.duration }));
    const handle = setInterval(
      () =>
        setClockValues((v) => ({
          ...v,
          [id]: (v[id] ?? capture.duration) + 1,
        })),
      1000,
    );
    return () => clearInterval(handle);
  }, [capture?.id, capture?.status]);
  useEffect(() => {
    if (tab === "transcript" && recording && followLive)
      transcriptEnd.current?.scrollIntoView({
        behavior: "smooth",
        block: "end",
      });
  }, [meeting?.segments.length, recording, followLive, tab]);
  useEffect(() => {
    messageEnd.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [stream?.text, meeting?.messages.length]);
  useEffect(() => {
    const handle = (e: globalThis.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setPane("library");
        searchRef.current?.focus();
      }
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        setView("settings");
        setPane("document");
      }
      if (e.key === "Escape") {
        setShowExport(false);
        setShowMore(false);
        setRenameSpeaker(null);
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "j") {
        e.preventDefault();
        setAssistantOpen(true);
        setPane("assistant");
        promptRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handle);
    return () => window.removeEventListener("keydown", handle);
  }, []);
  const run = async (name: string, fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(name);
    const owner = selectedRef.current;
    if (owner) operationOwners.current.add(owner);
    try {
      await fn();
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
    } finally {
      if (owner) operationOwners.current.delete(owner);
      setMeetings((prev) => prev);
      setBusy(null);
    }
  };
  const choose = (id: string) => {
    selectedRef.current = id;
    detailUse.current.set(id, ++detailClock.current);
    setMeetings((prev) => prev);
    setSelectedId(id);
    setEditingRecap(false);
    setView("meeting");
    setPane("document");
    setTranscriptQuery("");
    setShowMore(false);
    setShowExport(false);
    setHighlight(null);
  };
  const create = () =>
    void run("create", async () => {
      const m = await call<Meeting>("create_meeting");
      merge(m);
      choose(m.id);
      setTab("notes");
    });
  const loadExample = () =>
    void run("example", async () => {
      const m = await call<Meeting>("load_example");
      merge(m);
      choose(m.id);
      setTab("summary");
    });
  const update = async (id: string, changes: MeetingChanges) => {
    bumpDetail(id);
    setMeetings((prev) =>
      prev.map((m) => (m.id === id ? { ...m, ...changes } : m)),
    );
    try {
      const saved = await call<Meeting>("update_meeting", { id, changes });
      library.upsert(meetingHeader(saved));
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
      throw e;
    }
  };
  const saveNotes = useCallback(
    (id: string, notes: string): Promise<boolean> => {
      // Keep writes for one meeting in order, including a close-time flush.
      const previous = noteSaves.current.get(id) ?? Promise.resolve(true);
      const next = previous
        .catch(() => false)
        .then(async () => {
          try {
            const saved = await call<Meeting>("update_meeting", {
              id,
              changes: { notes },
            });
            library.upsert(meetingHeader(saved));
            if (pendingNotes.current.get(id) === notes) {
              pendingNotes.current.delete(id);
              setMeetings((prev) => prev);
              setNoteStates((v) => ({ ...v, [id]: "saved" }));
            }
            return true;
          } catch {
            if (pendingNotes.current.get(id) === notes) {
              setNoteStates((v) => ({ ...v, [id]: "error" }));
              notify(
                "Your latest notes are still in this window, but could not be saved. Keep Tuck open and choose Retry save.",
                true,
              );
            }
            return false;
          }
        });
      noteSaves.current.set(id, next);
      void next.then(() => {
        if (noteSaves.current.get(id) === next) noteSaves.current.delete(id);
      });
      return next;
    },
    [notify, library.upsert, setMeetings],
  );
  const saveRecap = useCallback(
    (id: string, summary: string): Promise<boolean> => {
      const previous = recapSaves.current.get(id) ?? Promise.resolve(true);
      const next = previous
        .catch(() => false)
        .then(async () => {
          try {
            const saved = await call<Meeting>("update_meeting", {
              id,
              changes: { summary },
            });
            library.upsert(meetingHeader(saved));
            if (pendingRecaps.current.get(id) === summary) {
              pendingRecaps.current.delete(id);
              setMeetings((prev) => prev);
              setRecapStates((v) => ({ ...v, [id]: "saved" }));
            }
            return true;
          } catch {
            if (pendingRecaps.current.get(id) === summary) {
              setRecapStates((v) => ({ ...v, [id]: "error" }));
              notify(
                "Your recap is still in this window. Keep Tuck open and retry saving.",
                true,
              );
            }
            return false;
          }
        });
      recapSaves.current.set(id, next);
      void next.then(() => {
        if (recapSaves.current.get(id) === next) recapSaves.current.delete(id);
      });
      return next;
    },
    [notify, library.upsert, setMeetings],
  );
  const changeRecap = (id: string, summary: string) => {
    bumpDetail(id);
    setMeetings((prev) =>
      prev.map((m) => (m.id === id ? { ...m, summary } : m)),
    );
    pendingRecaps.current.set(id, summary);
    setRecapStates((v) => ({ ...v, [id]: "saving" }));
    const timer = recapTimers.current.get(id);
    if (timer) clearTimeout(timer);
    recapTimers.current.set(
      id,
      setTimeout(() => void saveRecap(id, summary), 500),
    );
  };
  const flushEdits = useCallback(
    async (id?: string) => {
      const correctionsSaved = () => {
        const draft = [...pendingCorrections.current.values()].find(
          (d) => (!id || d.meetingId === id) && d.text !== d.baseText,
        );
        if (!draft) return true;
        setSelectedId(draft.meetingId);
        setView("meeting");
        setPane("document");
        setTab("transcript");
        setTranscriptQuery("");
        setHighlight(draft.segmentId);
        setTimeout(
          () =>
            document
              .getElementById(`segment-${draft.segmentId}`)
              ?.scrollIntoView({ block: "center", behavior: "smooth" }),
          60,
        );
        notify(
          "Save or cancel your transcript correction before continuing. Your draft is still here.",
          true,
        );
        return false;
      };
      if (!correctionsSaved()) return false;
      for (const [key, timer] of noteTimers.current)
        if (!id || key === id) clearTimeout(timer);
      for (const [key, timer] of recapTimers.current)
        if (!id || key === id) clearTimeout(timer);
      const saved = await Promise.all(
        [...pendingNotes.current.entries()]
          .filter(([key]) => !id || key === id)
          .map(([key, text]) => saveNotes(key, text))
          .concat(
            [...pendingRecaps.current.entries()]
              .filter(([key]) => !id || key === id)
              .map(([key, text]) => saveRecap(key, text)),
          ),
      );
      return (
        saved.every(Boolean) &&
        correctionsSaved() &&
        (id
          ? !pendingNotes.current.has(id) && !pendingRecaps.current.has(id)
          : !pendingNotes.current.size && !pendingRecaps.current.size)
      );
    },
    [saveNotes, saveRecap, notify],
  );
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let closing = false;
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        const current = getCurrentWindow();
        const stop = await current.onCloseRequested(async (event) => {
          event.preventDefault();
          if (closing || desktopClosing.current) return;
          closing = true;
          desktopClosing.current = true;
          try {
            if (await flushEdits()) await call("hide_window");
          } catch {
            notify(
              "Your edits could not be saved. Keep Tuck open and retry saving.",
              true,
            );
          } finally {
            closing = false;
            desktopClosing.current = false;
          }
        });
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() =>
        notify(
          "Closing protection is unavailable. Save your notes before closing Tuck.",
          true,
        ),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [flushEdits, notify]);
  const changeNotes = (id: string, notes: string) => {
    bumpDetail(id);
    setMeetings((prev) => prev.map((m) => (m.id === id ? { ...m, notes } : m)));
    pendingNotes.current.set(id, notes);
    setNoteStates((v) => ({ ...v, [id]: "saving" }));
    const timer = noteTimers.current.get(id);
    if (timer) clearTimeout(timer);
    noteTimers.current.set(
      id,
      setTimeout(() => void saveNotes(id, notes), 500),
    );
  };
  useEffect(() => {
    const before = (e: BeforeUnloadEvent) => {
      if (
        pendingNotes.current.size ||
        pendingRecaps.current.size ||
        [...pendingCorrections.current.values()].some(
          (d) => d.text !== d.baseText,
        )
      ) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", before);
    return () => window.removeEventListener("beforeunload", before);
  }, []);
  const updateSettings = async (changes: Partial<SettingsData>) => {
    const result = await call<SettingsData>("update_settings", { changes });
    setSettings({ ...defaults, ...result });
  };
  const start = () => {
    if (!meeting) return;
    void run("record", async () => {
      const m = await call<Meeting>("start_recording", {
        meetingId: meeting.id,
        microphone,
        systemAudio,
      });
      merge(m);
      setClockValues((v) => ({ ...v, [m.id]: m.duration }));
      setTab("transcript");
      setFollowLive(true);
      await refreshStatus();
    });
  };
  const control = (method: string) => {
    if (!meeting) return;
    void run(method, async () => {
      const m = await call<Meeting>(method, { meetingId: meeting.id });
      merge(m);
      await refreshStatus();
      if (method === "stop_recording") {
        setClockValues((v) => ({ ...v, [m.id]: m.duration }));
        notify("Conversation saved on this device.");
      }
    });
  };
  const importAudio = () =>
    void run("import", async () => {
      const path = await selectAudio();
      if (!path) return;
      notify("Preparing and transcribing your audio locally…");
      const m = await call<Meeting>("import_audio", { path });
      merge(m);
      choose(m.id);
      setTab("transcript");
      notify("Audio imported and transcribed.");
    });
  const jump = (time: number) => {
    if (!meeting) return;
    audioSeek.current?.(time);
    const ordered = [...meeting.segments].sort((a, b) => a.start - b.start);
    const match =
      ordered.find((s) => s.start <= time && s.end >= time) ??
      [...ordered].reverse().find((s) => s.start <= time) ??
      ordered[0];
    setTab("transcript");
    setPane("document");
    setTranscriptQuery("");
    setFollowLive(false);
    if (match) {
      setHighlight(match.id);
      setTimeout(
        () =>
          document
            .getElementById(`segment-${match.id}`)
            ?.scrollIntoView({ behavior: "smooth", block: "center" }),
        60,
      );
    }
  };
  const summarize = () => {
    if (!meeting) return;
    void run("summary", async () => {
      if (!(await flushEdits(meeting.id)))
        throw new Error(
          "Save your latest notes before creating a recap. Choose Retry save in Notes.",
        );
      const m = await call<Meeting>("summarize", { meetingId: meeting.id });
      merge(m);
      setTab("summary");
      notify("Summary saved.");
    });
  };
  const publish = () => {
    if (!meeting) return;
    void run("notion", async () => {
      if (!(await flushEdits(meeting.id)))
        throw new Error(
          "Save your latest notes before sending them to Notion. Choose Retry save in Notes.",
        );
      if (meeting.notionUrl) {
        await openExternal(meeting.notionUrl);
        return;
      }
      const m = await call<Meeting>("publish_notion", {
        meetingId: meeting.id,
      });
      merge(m);
      notify("Notes published to Notion.");
    });
  };
  const syncNotion = () => {
    if (!meeting) return;
    void run("notion-update", async () => {
      if (!(await flushEdits(meeting.id)))
        throw new Error("Save your latest edits before updating Notion.");
      merge(await call<Meeting>("sync_notion", { meetingId: meeting.id }));
      notify("Notion notes updated.");
    });
  };
  const exportMeeting = (format: ExportFormat) => {
    if (!meeting) return;
    setShowExport(false);
    void run("export", async () => {
      if (!(await flushEdits(meeting.id)))
        throw new Error(
          "Save your latest notes before exporting. Choose Retry save in Notes.",
        );
      if (await saveExport(meeting, format)) notify("Conversation exported.");
    });
  };
  const remove = () => {
    const id = deleteId;
    if (!id) return;
    setDeleteId(null);
    void run("delete", async () => {
      if (!(await flushEdits(id)))
        throw new Error("Save your latest edits before deleting this meeting.");
      await call("delete_meeting", { id });
      pendingNotes.current.delete(id);
      const timer = noteTimers.current.get(id);
      if (timer) clearTimeout(timer);
      bumpDetail(id);
      library.remove(id);
      setMeetings((prev) => prev.filter((m) => m.id !== id));
      if (selectedId === id)
        setSelectedId(library.headers.find((m) => m.id !== id)?.id ?? null);
      notify("Conversation deleted from this device.");
    });
  };
  const ask = async (text = prompt) => {
    if (!meeting || !text.trim() || chatBusy) return;
    const id = meeting.id;
    const requestId = crypto.randomUUID();
    const optimistic: Message = {
      id: `pending-${requestId}`,
      role: "user",
      content: text.trim(),
      createdAt: new Date().toISOString(),
    };
    setPrompt("");
    setChatBusy(id);
    chatRequest.current = requestId;
    setStream({ meetingId: id, requestId, text: "" });
    setMeetings((prev) =>
      prev.map((m) =>
        m.id === id ? { ...m, messages: [...m.messages, optimistic] } : m,
      ),
    );
    try {
      const result = await call<Message>("chat", {
        meetingId: id,
        prompt: text.trim(),
        requestId,
      });
      setMeetings((prev) =>
        prev.map((m) =>
          m.id === id
            ? {
                ...m,
                messages: [
                  ...m.messages.filter((msg) => msg.id !== result.id),
                  result,
                ],
              }
            : m,
        ),
      );
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
      setMeetings((prev) =>
        prev.map((m) =>
          m.id === id
            ? {
                ...m,
                messages: m.messages.filter((msg) => msg.id !== optimistic.id),
              }
            : m,
        ),
      );
      setPrompt((p) => p || text);
    } finally {
      setChatBusy(null);
      chatRequest.current = null;
      setStream(null);
    }
  };
  desktopIntent.current = async (intent: string) => {
    if (intent === "hide" || intent === "quit") {
      if (desktopClosing.current) return;
      desktopClosing.current = true;
      try {
        if (await flushEdits())
          await call(intent === "quit" ? "quit_app" : "hide_window");
      } finally {
        desktopClosing.current = false;
      }
      return;
    }
    if (intent === "record") {
      if (quickCapturePending.current) return;
      if (capture || status.activeMeetingId) {
        choose(capture?.id || status.activeMeetingId!);
        return;
      }
      if (!microphone && !systemAudio) {
        notify(
          "Select your microphone or computer audio before recording.",
          true,
        );
        return;
      }
      quickCapturePending.current = true;
      try {
        const readiness = await call<{
          modelReady: boolean;
          helperReady: boolean;
          microphoneAvailable: boolean;
          systemAvailable: boolean;
        }>("capture_readiness");
        if (
          !readiness.modelReady ||
          !readiness.helperReady ||
          (microphone && !readiness.microphoneAvailable) ||
          (systemAudio && !readiness.systemAvailable)
        ) {
          setSettingsSection("Audio & models");
          setView("settings");
          setPane("document");
          notify("Check your recording setup before starting.");
          return;
        }
        const m = await call<Meeting>("create_meeting");
        merge(m);
        choose(m.id);
        const started = await call<Meeting>("start_recording", {
          meetingId: m.id,
          microphone,
          systemAudio,
        });
        merge(started);
        setTab("transcript");
        await refreshStatus();
      } catch (e) {
        notify(e instanceof Error ? e.message : String(e), true);
      } finally {
        quickCapturePending.current = false;
      }
    }
  };
  const sourceIntent = useRef(0);
  const openSource = (url: string) => {
    const intent = ++sourceIntent.current;
    try {
      const parsed = new URL(url);
      const id = decodeURIComponent(parsed.pathname.replace(/^\//, ""));
      choose(id);
      const source = parsed.searchParams.get("source");
      setTab(
        source === "notes"
          ? "notes"
          : source === "summary"
            ? "summary"
            : source === "tasks"
              ? "tasks"
              : "transcript",
      );
      void loadMeeting(id)
        .then((sourceMeeting) => {
          if (intent !== sourceIntent.current || selectedRef.current !== id)
            return;
          const segment =
            sourceMeeting.segments.find(
              (s) => s.id === parsed.searchParams.get("segment"),
            ) ||
            sourceMeeting.segments.find(
              (s) => s.start >= Number(parsed.searchParams.get("start") || 0),
            );
          if (segment && (!source || source === "transcript")) {
            setHighlight(segment.id);
            setFollowLive(false);
            setTimeout(() => {
              if (selectedRef.current === id)
                document
                  .getElementById(`segment-${segment.id}`)
                  ?.scrollIntoView({ behavior: "smooth", block: "center" });
            }, 80);
          }
        })
        .catch((e) => {
          if (intent === sourceIntent.current && selectedRef.current === id)
            setDetailError({
              id,
              text: e instanceof Error ? e.message : String(e),
            });
        });
    } catch {
      notify("This source link could not be opened.", true);
    }
  };
  const shown = library.headers;
  const groups = [...new Set(shown.map((m) => relativeGroup(m.createdAt)))];
  const visibleSegments =
    meeting?.segments.filter(
      (s) =>
        !transcriptQuery ||
        s.text.toLowerCase().includes(transcriptQuery.toLowerCase()) ||
        meeting.speakers
          .find((p) => p.id === s.speakerId)
          ?.name.toLowerCase()
          .includes(transcriptQuery.toLowerCase()),
    ) ?? [];

  if (loading)
    return (
      <div className="startup">
        <Brand />
        <LoaderCircle size={20} className="spin" />
        <p>Opening your local workspace…</p>
      </div>
    );
  if (bootError)
    return (
      <div className="startup">
        <Brand />
        <h1>The local engine needs attention.</h1>
        <p className="startup-error">{bootError}</p>
        <p>Your saved meetings stay on this computer.</p>
        <button className="button primary" onClick={() => void bootstrap()}>
          <ArrowRight size={16} />
          Try again
        </button>
      </div>
    );

  return (
    <div
      className={`app-shell mobile-${pane}${!assistantOpen ? " assistant-hidden" : ""}${view !== "meeting" ? " settings-active" : ""}`}
    >
      <aside className="sidebar" aria-label="Conversation library">
        <div className="sidebar-tools">
          <div className="sidebar-brand">
            <Brand />
            {!native && <span className="preview-badge">PREVIEW</span>}
            <button
              className="icon-button mobile-only"
              aria-label="Close library"
              onClick={() => setPane("document")}
            >
              <X size={18} />
            </button>
          </div>
          <button
            className="new-conversation"
            onClick={create}
            disabled={!!busy}
          >
            <Plus size={17} />
            <span>New meeting</span>
            <span className="new-shortcut" aria-hidden="true">
              +
            </span>
          </button>
          <label className="library-search">
            <Search size={15} />
            <input
              ref={searchRef}
              aria-label="Search conversations"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search anything…"
            />
            <kbd>⌘ K</kbd>
          </label>
          <nav className="library-nav" aria-label="Library filters">
            <button
              className={
                filter === "all" && view === "meeting" ? "selected" : ""
              }
              onClick={() => {
                setFilter("all");
                setView("meeting");
              }}
            >
              <Library size={16} />
              <span>All conversations</span>
              <span className="count">{library.overview.meetingCount}</span>
            </button>
            <button
              className={
                filter === "starred" && view === "meeting" ? "selected" : ""
              }
              onClick={() => {
                setFilter("starred");
                setView("meeting");
              }}
            >
              <Star size={16} />
              <span>Starred</span>
              {library.overview.starredCount > 0 && (
                <span className="count">{library.overview.starredCount}</span>
              )}
            </button>
            <button
              className={view === "library" ? "selected" : ""}
              onClick={() => {
                setView("library");
                setPane("document");
              }}
            >
              <Search size={16} />
              <span>Ask your library</span>
            </button>
            <button
              className={view === "tasks" ? "selected" : ""}
              onClick={() => {
                setView("tasks");
                setPane("document");
              }}
            >
              <Check size={16} />
              <span>Tasks</span>
              <span className="count">{library.overview.openTaskCount}</span>
            </button>
          </nav>
          <ScopeFilters
            library={library.overview}
            scope={scope}
            onChange={setScope}
          />
        </div>
        <div className="meeting-list">
          <UpcomingMeetings merge={merge} choose={choose} notify={notify} />

          {groups.map((group) => (
            <section key={group}>
              <h2>{group}</h2>
              {shown
                .filter((m) => relativeGroup(m.createdAt) === group)
                .map((m) => (
                  <button
                    key={m.id}
                    className={`meeting-item${selectedId === m.id && view === "meeting" ? " selected" : ""}`}
                    onClick={() => choose(m.id)}
                  >
                    <div className="meeting-item-top">
                      <span
                        className={`meeting-state-icon ${m.status === "recording" ? "recording" : ""}`}
                      >
                        {m.status === "recording" ? (
                          <AudioLines size={16} />
                        ) : (
                          <FileText size={15} />
                        )}
                      </span>
                      <span className="meeting-item-title">{m.title}</span>
                      {m.starred && <Star className="filled-star" size={12} />}
                    </div>
                    <span className="meeting-item-meta">
                      {m.isExample
                        ? "Example conversation"
                        : m.status === "recording"
                          ? "Recording now"
                          : m.status === "paused"
                            ? "Paused"
                            : m.status === "processing"
                              ? "Finishing transcript…"
                              : `${meetingDate(m.createdAt)} · ${duration(m.duration)}`}
                    </span>
                  </button>
                ))}
            </section>
          ))}
          {library.loading && (
            <p className="library-load-state" role="status">
              <LoaderCircle size={14} className="spin" />
              Loading conversations…
            </p>
          )}
          {library.notice && (
            <p className="library-load-state" role="status">
              {library.notice}
            </p>
          )}
          {library.error && (
            <div className="library-load-state" role="alert">
              <p>{library.error}</p>
              <button className="text-link" onClick={() => void library.load()}>
                Try again
              </button>
            </div>
          )}
          {library.cursor && !library.loading && (
            <button
              className="library-load-more"
              onClick={() => void library.load(true)}
            >
              Load more conversations
            </button>
          )}
          {!shown.length && !library.loading && !library.error && (
            <div className="library-empty">
              <Folder size={22} strokeWidth={1.4} />
              <p>
                {query
                  ? "No conversations found."
                  : filter === "starred"
                    ? "Your favorites belong here."
                    : "A little space for your conversations."}
              </p>
              {query && (
                <button className="text-link" onClick={() => setQuery("")}>
                  Clear search
                </button>
              )}
            </div>
          )}
        </div>
        <div className="sidebar-bottom">
          <button
            className="sidebar-link"
            onClick={importAudio}
            disabled={!!busy}
          >
            <Upload size={16} />
            <span>
              {busy === "import" ? "Importing audio…" : "Import audio"}
            </span>
          </button>
          <button
            className={`sidebar-link${view === "settings" ? " selected" : ""}`}
            onClick={() => {
              setView("settings");
              setPane("document");
            }}
          >
            <Settings2 size={16} />
            <span>Settings</span>
            <kbd>⌘ ,</kbd>
          </button>
          <div className="local-workspace">
            <span className="local-lock">
              <ShieldCheck size={17} />
            </span>
            <div>
              <strong>Local workspace</strong>
              <small>
                {native
                  ? "Stored on this device"
                  : "Illustrative browser preview"}
              </small>
            </div>
            <span className="status-dot" />
          </div>
        </div>
      </aside>
      <main className="workspace">
        <div className="workspace-topbar">
          <div>
            <button
              className="icon-button library-toggle"
              aria-label="Open conversation library"
              onClick={() => setPane("library")}
            >
              <Menu size={18} />
            </button>
            <span>
              {view === "settings"
                ? "Settings"
                : view === "library"
                  ? "Ask your library"
                  : view === "tasks"
                    ? "Tasks"
                    : "Conversations"}
            </span>
            {meeting && view === "meeting" && (
              <>
                <ChevronRight size={13} />
                <span className="breadcrumb-title">{meeting.title}</span>
              </>
            )}
          </div>
          <div className="topbar-actions">
            {native ? (
              <span className="local-status">
                <span className="status-dot" />
                On-device transcription
              </span>
            ) : (
              <span className="preview-status">Desktop interface preview</span>
            )}
            {view === "meeting" && (
              <button
                className={`icon-button ${assistantOpen ? "is-active" : ""}`}
                aria-label={assistantOpen ? "Hide assistant" : "Show assistant"}
                title="Assistant · ⌘ J"
                onClick={() => {
                  setAssistantOpen((v) => !v);
                  setPane("assistant");
                }}
              >
                <MessageSquare size={17} />
              </button>
            )}
          </div>
        </div>
        {view === "settings" ? (
          <div className="document-scroll settings-scroll">
            <Settings
              initialTab={settingsSection}
              settings={settings}
              accounts={accounts}
              status={status}
              onUpdate={updateSettings}
              onAccounts={setAccounts}
              onStatus={refreshStatus}
              notify={notify}
              meetingCount={library.overview.meetingCount}
              beforeFileOperation={() => flushEdits()}
              onLibraryChanged={async () => {
                const data = await call<LibraryBootstrap>("bootstrap_library", {
                  limit: 50,
                });
                setMeetings([]);
                detailVersions.current.clear();
                library.install(data);
                setSettings({ ...defaults, ...data.settings });
                setStatus(data.status);
                setAccounts(data.accounts);
              }}
            />
          </div>
        ) : view === "library" || view === "tasks" ? (
          <div className="document-scroll">
            <LibraryDesk
              key={JSON.stringify(scope)}
              mode={view}
              library={library.overview}
              revision={library.revision}
              settings={settings}
              scope={scope}
              onSource={openSource}
              merge={merge}
              notify={notify}
            />
          </div>
        ) : selectedId && !meeting ? (
          <div
            className="meeting-load-state"
            role={detailError?.id === selectedId ? "alert" : "status"}
          >
            {detailError?.id === selectedId ? (
              <>
                <h2>This conversation could not be opened.</h2>
                {detailError.text !==
                  "This conversation could not be opened." && (
                  <p>{detailError.text}</p>
                )}
                <button
                  className="button secondary compact"
                  onClick={() => {
                    setDetailError(null);
                    setDetailLoading(selectedId);
                    void loadMeeting(selectedId, true)
                      .catch((e) =>
                        setDetailError({
                          id: selectedId,
                          text: e instanceof Error ? e.message : String(e),
                        }),
                      )
                      .finally(() => setDetailLoading(null));
                  }}
                >
                  Try again
                </button>
              </>
            ) : (
              <>
                <LoaderCircle size={22} className="spin" />
                <p>
                  {detailLoading === selectedId
                    ? "Opening conversation…"
                    : "Opening conversation…"}
                </p>
              </>
            )}
          </div>
        ) : !meeting ? (
          <div className="welcome">
            <div className="welcome-symbol">
              <AudioLines size={39} strokeWidth={1.3} />
              <span className="welcome-orbit">
                <ShieldCheck size={15} />
              </span>
            </div>
            <h1>
              Stay in the
              <br />
              conversation.
            </h1>
            <p>
              Tuck takes care of the words.
              <br />
              Private transcripts, useful notes, and a thinking partner when you
              need one.
            </p>
            <button
              className="button primary"
              onClick={create}
              disabled={!!busy}
            >
              <Plus size={17} />
              Start a meeting
            </button>
            <button
              className="welcome-example"
              onClick={loadExample}
              disabled={!!busy}
            >
              <BookOpen size={15} />
              Open an example
              <ArrowRight size={14} />
            </button>
            <div className="welcome-basics">
              <div>
                <Mic size={16} />
                <span>Mic + system audio</span>
              </div>
              <div>
                <ShieldCheck size={16} />
                <span>No meeting bot</span>
              </div>
              <div>
                <HardDriveIcon />
                <span>On your device</span>
              </div>
            </div>
            {!native && (
              <p className="preview-explanation">
                You're in a browser preview. Recording and AI run in the desktop
                app.
              </p>
            )}
          </div>
        ) : (
          <>
            <div className="document-scroll">
              <article className="meeting-document">
                <div className="meeting-heading-top">
                  <span
                    className={`meeting-label ${recording ? "live-label" : ""}`}
                  >
                    {recording ? (
                      <>
                        <span className="record-dot" />
                        Recording
                      </>
                    ) : meeting.status === "paused" ? (
                      <>
                        <Pause size={12} />
                        Paused
                      </>
                    ) : meeting.isExample ? (
                      <>
                        <BookOpen size={12} />
                        Example conversation
                      </>
                    ) : (
                      <>
                        <AudioLines size={13} />
                        Conversation
                      </>
                    )}
                  </span>
                  <div className="meeting-utilities">
                    <button
                      className={`icon-button${meeting.starred ? " starred" : ""}`}
                      aria-label={
                        meeting.starred
                          ? "Unstar conversation"
                          : "Star conversation"
                      }
                      title={
                        meeting.starred
                          ? "Unstar conversation"
                          : "Star conversation"
                      }
                      onClick={() =>
                        void update(meeting.id, {
                          starred: !meeting.starred,
                        }).catch(() => {})
                      }
                    >
                      <Star size={16} />
                    </button>
                    <div className="popover-anchor">
                      <button
                        className="icon-button"
                        aria-label="Export conversation"
                        title="Export conversation"
                        aria-expanded={showExport}
                        onClick={() => {
                          setShowExport((v) => !v);
                          setShowMore(false);
                        }}
                      >
                        <Download size={16} />
                      </button>
                      {showExport && (
                        <div className="popover-menu" role="menu">
                          <span className="menu-label">
                            Export conversation
                          </span>
                          {(["markdown", "srt", "json"] as ExportFormat[]).map(
                            (f) => (
                              <button
                                role="menuitem"
                                key={f}
                                onClick={() => exportMeeting(f)}
                              >
                                <FileText size={15} />
                                {f === "markdown"
                                  ? "Markdown"
                                  : f === "srt"
                                    ? "Subtitles (.srt)"
                                    : "JSON data"}
                              </button>
                            ),
                          )}
                        </div>
                      )}
                    </div>
                    <div className="popover-anchor">
                      <button
                        className="icon-button"
                        aria-label="More conversation actions"
                        aria-expanded={showMore}
                        onClick={() => {
                          setShowMore((v) => !v);
                          setShowExport(false);
                        }}
                      >
                        <MoreHorizontal size={18} />
                      </button>
                      {showMore && (
                        <div className="popover-menu right-menu" role="menu">
                          <button
                            role="menuitem"
                            disabled={
                              !!active || meeting.status === "processing"
                            }
                            className="destructive"
                            onClick={() => {
                              setDeleteId(meeting.id);
                              setShowMore(false);
                            }}
                          >
                            <Trash2 size={15} />
                            Delete conversation
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
                <input
                  className="meeting-title"
                  aria-label="Conversation title"
                  value={meeting.title}
                  maxLength={200}
                  onChange={(e) => {
                    bumpDetail(meeting.id);
                    setMeetings((prev) =>
                      prev.map((m) =>
                        m.id === meeting.id
                          ? { ...m, title: e.target.value }
                          : m,
                      ),
                    );
                  }}
                  onBlur={(e) => {
                    const title =
                      e.target.value.trim() || "Untitled conversation";
                    void update(meeting.id, { title }).catch(() => {});
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                  }}
                />
                <div className="meeting-metadata">
                  <span>{fullDate(meeting.createdAt)}</span>
                  <span className="metadata-separator">·</span>
                  <span>
                    <Clock size={13} />
                    {active ? clock(elapsed) : duration(meeting.duration)}
                  </span>
                  {meeting.speakers.length > 0 && (
                    <>
                      <span className="metadata-separator">·</span>
                      <span>
                        <Users size={13} />
                        {meeting.speakers.length} speakers
                      </span>
                    </>
                  )}
                </div>
                <Organization
                  key={meeting.id}
                  meeting={meeting}
                  update={(changes) => update(meeting.id, changes)}
                />
                {meeting.isExample && (
                  <div className="example-note">
                    <BookOpen size={13} />
                    <span>
                      This is an illustrative conversation. Try notes, speaker
                      names, search, and exports.
                    </span>
                  </div>
                )}
                {meeting.recoveredFrom && (
                  <div className="recovered-note">
                    <AudioLines size={14} />
                    <span>
                      Recovered into a separate meeting. The original was kept.
                    </span>
                    {meeting.recoveredFrom && (
                      <button
                        className="text-link"
                        onClick={() => choose(meeting.recoveredFrom!)}
                      >
                        Open original
                      </button>
                    )}
                  </div>
                )}
                {meeting.status === "interrupted" && (
                  <div className="interrupted-note">
                    <ShieldCheck size={16} />
                    <div>
                      <strong>Recording was interrupted.</strong>
                      <p>
                        Your saved transcript and notes are here. If a recording
                        was retained, recover it into a separate meeting.
                      </p>
                      <button
                        className="text-link"
                        onClick={() => {
                          setRecoverRequested(meeting.id);
                          setTab("transcript");
                        }}
                      >
                        Recover saved audio <ArrowRight size={13} />
                      </button>
                    </div>
                  </div>
                )}
                {(meeting.status === "draft" || active) && (
                  <div
                    className={`recording-toolbar${active ? " capturing" : ""}`}
                  >
                    <div className="audio-sources">
                      <button
                        className={microphone ? "source-on" : ""}
                        aria-pressed={microphone}
                        disabled={!!active}
                        onClick={() => setMicrophone((v) => !v)}
                        title="Include your microphone"
                      >
                        <Mic size={16} />
                        <span>You</span>
                        <span className="source-check">
                          {microphone ? <Check size={10} /> : <X size={10} />}
                        </span>
                      </button>
                      <button
                        className={systemAudio ? "source-on" : ""}
                        aria-pressed={systemAudio}
                        disabled={!!active}
                        onClick={() => setSystemAudio((v) => !v)}
                        title="Include meeting audio from your computer"
                      >
                        <Monitor size={16} />
                        <span>Computer audio</span>
                        <span className="source-check">
                          {systemAudio ? <Check size={10} /> : <X size={10} />}
                        </span>
                      </button>
                    </div>
                    {active ? (
                      <div className="recording-controls">
                        <span className="recording-time">
                          <span
                            className={recording ? "record-dot" : "paused-dot"}
                          />
                          {clock(elapsed)}
                        </span>
                        <button
                          className="icon-button"
                          aria-label={
                            recording ? "Pause recording" : "Resume recording"
                          }
                          disabled={!!busy}
                          onClick={() =>
                            control(
                              recording
                                ? "pause_recording"
                                : "resume_recording",
                            )
                          }
                        >
                          {recording ? <Pause size={16} /> : <Play size={16} />}
                        </button>
                        <button
                          className="button stop-button compact"
                          disabled={!!busy}
                          onClick={() => control("stop_recording")}
                        >
                          {busy === "stop_recording" ? (
                            <LoaderCircle size={13} className="spin" />
                          ) : (
                            <Square size={11} fill="currentColor" />
                          )}
                          Finish
                        </button>
                      </div>
                    ) : (
                      <button
                        className="button primary compact"
                        disabled={
                          !!busy ||
                          (!microphone && !systemAudio) ||
                          !!capture ||
                          !!status.activeMeetingId
                        }
                        onClick={
                          status.modelInstalled
                            ? start
                            : () => {
                                setSettingsSection("Audio & models");
                                setView("settings");
                                setPane("document");
                              }
                        }
                      >
                        {busy === "record" ? (
                          <LoaderCircle size={14} className="spin" />
                        ) : (
                          <span className="button-record-dot" />
                        )}
                        {status.modelInstalled ? "Start recording" : "Set up"}
                      </button>
                    )}
                  </div>
                )}
                {meeting.status === "draft" && !status.modelInstalled && (
                  <div className="setup-hint">
                    <CpuIcon />
                    <span>
                      Before your first recording, download the local speech
                      model.
                    </span>
                    <button
                      onClick={() => {
                        setSettingsSection("Audio & models");
                        setView("settings");
                        setPane("document");
                      }}
                    >
                      Set up private transcription <ArrowRight size={13} />
                    </button>
                  </div>
                )}
                {meeting.status === "processing" && (
                  <div className="processing-bar" role="status">
                    <LoaderCircle className="spin" size={15} />
                    <span>
                      Finishing the transcript. Your conversation is saved
                      locally.
                    </span>
                  </div>
                )}
                <div className="document-tabs-row">
                  <div
                    className="document-tabs"
                    onKeyDown={tabNavigation}
                    role="tablist"
                    aria-label="Conversation views"
                  >
                    {[
                      { id: "summary", name: "Recap", Icon: Sparkles },
                      { id: "notes", name: "Notes", Icon: FileText },
                      { id: "tasks", name: "Tasks", Icon: Check },
                      {
                        id: "transcript",
                        name: "Transcript",
                        Icon: AudioLines,
                      },
                    ].map(({ id, name, Icon }) => (
                      <button
                        role="tab"
                        key={id}
                        aria-selected={tab === id}
                        className={tab === id ? "selected" : ""}
                        onClick={() => setTab(id as Tab)}
                      >
                        <Icon size={15} />
                        {name}
                        {id === "transcript" && meeting.segments.length > 0 && (
                          <span className="tab-count">
                            {meeting.segments.length}
                          </span>
                        )}
                      </button>
                    ))}
                  </div>
                  {tab === "notes" && (
                    <span
                      className={`save-state ${noteStates[meeting.id] === "error" ? "save-error" : ""}`}
                    >
                      {noteStates[meeting.id] === "saving" ? (
                        <>
                          <LoaderCircle size={11} className="spin" />
                          Saving…
                        </>
                      ) : noteStates[meeting.id] === "error" ? (
                        <button
                          onClick={() =>
                            void saveNotes(meeting.id, meeting.notes)
                          }
                        >
                          Retry save
                        </button>
                      ) : (
                        <>
                          <Check size={12} />
                          Saved locally
                        </>
                      )}
                    </span>
                  )}
                </div>
                {tab === "tasks" && (
                  <Tasks
                    key={meeting.id}
                    meeting={meeting}
                    merge={merge}
                    notify={notify}
                  />
                )}
                {tab === "summary" && (
                  <section
                    className="summary-panel"
                    role="tabpanel"
                    aria-label="Summary"
                  >
                    <div className="summary-toolbar">
                      <label>
                        <span className="visually-hidden">
                          Summary template
                        </span>
                        <select
                          value={meeting.template}
                          onChange={(e) =>
                            void update(meeting.id, {
                              template: e.target.value,
                            }).catch(() => {})
                          }
                        >
                          <option value="general">Meeting notes</option>
                          <option value="sales">Sales discovery</option>
                          <option value="standup">Team standup</option>
                          <option value="interview">Interview</option>
                          <option value="one-on-one">One-on-one</option>
                          {settings.customTemplates.map((t) => (
                            <option value={t.id} key={t.id}>
                              {t.name}
                            </option>
                          ))}
                        </select>
                      </label>
                      <button
                        className="button subtle compact"
                        disabled={
                          !!busy ||
                          !meeting.segments.length ||
                          (settings.provider !== "local" &&
                            !settings.hostedConsent)
                        }
                        onClick={summarize}
                      >
                        {busy === "summary" ? (
                          <LoaderCircle size={14} className="spin" />
                        ) : (
                          <Sparkles size={14} />
                        )}{" "}
                        {busy === "summary"
                          ? "Writing recap…"
                          : meeting.summary
                            ? "Update recap"
                            : "Create recap"}
                      </button>
                    </div>
                    {settings.provider !== "local" && (
                      <div className="recap-processing-note">
                        <ShieldCheck size={12} />
                        <span>
                          {settings.hostedConsent
                            ? "Transcript context goes to OpenAI when you create or update this recap."
                            : "Allow transcript processing in Settings before creating a recap."}
                        </span>
                        {!settings.hostedConsent && (
                          <button
                            onClick={() => {
                              setSettingsSection("Intelligence");
                              setView("settings");
                              setPane("document");
                            }}
                          >
                            Open Settings <ArrowRight size={12} />
                          </button>
                        )}
                      </div>
                    )}
                    <div className="recap-edit-row">
                      <button
                        className="text-link"
                        disabled={busy === "summary"}
                        onClick={() => setEditingRecap(!editingRecap)}
                      >
                        {editingRecap ? "Read recap" : "Edit recap"}
                      </button>
                      <span className="save-state">
                        {recapStates[meeting.id] === "saving" ? (
                          "Saving…"
                        ) : recapStates[meeting.id] === "error" ? (
                          <button
                            onClick={() =>
                              void saveRecap(meeting.id, meeting.summary)
                            }
                          >
                            Retry recap save
                          </button>
                        ) : editingRecap ? (
                          "Saved locally"
                        ) : (
                          ""
                        )}
                      </span>
                    </div>
                    {editingRecap ? (
                      <textarea
                        disabled={busy === "summary"}
                        className="notes-editor recap-editor"
                        aria-label="Edit meeting recap"
                        value={meeting.summary}
                        onChange={(e) =>
                          changeRecap(meeting.id, e.target.value)
                        }
                      />
                    ) : meeting.summary ? (
                      <Markdown
                        text={meeting.summary}
                        onTimestamp={jump}
                        onSource={openSource}
                      />
                    ) : (
                      <div className="empty-summary">
                        <div className="summary-page-mark">
                          <Sparkles size={25} strokeWidth={1.4} />
                        </div>
                        <h2>The useful part, all together.</h2>
                        <p>
                          Turn the conversation into a clear summary, decisions,
                          and next steps. Your own notes help Tuck understand
                          what mattered.
                        </p>
                        {meeting.segments.length > 0 ? (
                          <button
                            className="button primary"
                            disabled={
                              !!busy ||
                              (settings.provider !== "local" &&
                                !settings.hostedConsent)
                            }
                            onClick={summarize}
                          >
                            <Sparkles size={15} />
                            Create recap
                          </button>
                        ) : (
                          <button
                            className="button secondary"
                            onClick={() => setTab("notes")}
                          >
                            <FileText size={15} />
                            Add your notes
                          </button>
                        )}
                        <span className="provider-footnote">
                          <ShieldCheck size={12} />
                          {settings.provider === "local"
                            ? "Recaps use your local model"
                            : `Transcript sent to ${settings.provider === "chatgpt" ? "your ChatGPT connection" : "OpenAI"} when you generate`}
                        </span>
                      </div>
                    )}
                    <WritingTools
                      key={`${meeting.id}:summary`}
                      meeting={meeting}
                      target="summary"
                      recipes={settings.recipes}
                      processingNote={
                        settings.provider === "local"
                          ? "Uses your local model."
                          : settings.hostedConsent
                            ? "Transcript and note context goes to OpenAI for this proposal."
                            : "Allow hosted transcript processing in Settings to continue."
                      }
                      allowed={
                        settings.provider === "local" || settings.hostedConsent
                      }
                      flush={() => flushEdits(meeting.id)}
                      merge={merge}
                      notify={notify}
                      onTimestamp={jump}
                    />
                    {meeting.summary && (
                      <div className="summary-footer">
                        <span>
                          <ShieldCheck size={13} />
                          {meeting.isExample
                            ? "Illustrative summary"
                            : "Recap · review important details"}
                        </span>
                        {meeting.notionUrl && (
                          <button
                            className="button secondary compact"
                            disabled={!!busy}
                            onClick={syncNotion}
                          >
                            {busy === "notion-update"
                              ? "Updating…"
                              : "Update notes"}
                          </button>
                        )}
                        <button
                          className="button secondary compact"
                          disabled={
                            !!busy ||
                            (meeting.notionStatus === "uncertain" &&
                              !meeting.notionUrl)
                          }
                          onClick={publish}
                        >
                          <NotionIcon />
                          {busy === "notion"
                            ? "Publishing…"
                            : meeting.notionUrl
                              ? "Open in Notion"
                              : "Send to Notion"}
                          <ArrowRight size={13} />
                        </button>
                      </div>
                    )}
                    {meeting.notionStatus === "uncertain" &&
                      !meeting.notionUrl && (
                        <NotionRecovery
                          key={meeting.id}
                          meetingId={meeting.id}
                          onRecovered={merge}
                          notify={notify}
                        />
                      )}
                    {meeting.notionStatus === "uncertain" && (
                      <p className="inline-error">
                        {meeting.notionUrl
                          ? "A previous update could not be confirmed. Use Update notes to reconcile this page safely."
                          : "Notion may have received this page. Check your database before trying again to avoid a duplicate."}
                      </p>
                    )}
                  </section>
                )}
                {tab === "notes" && (
                  <section
                    className="notes-panel"
                    role="tabpanel"
                    aria-label="My notes"
                  >
                    <div className="notes-intro">
                      <span>A little context goes a long way.</span>
                      <span>Only you decide what matters.</span>
                    </div>
                    <textarea
                      aria-label="Meeting notes"
                      className="notes-editor"
                      value={meeting.notes}
                      onChange={(e) => changeNotes(meeting.id, e.target.value)}
                      placeholder="Capture a thought, a question, a moment worth remembering…"
                      spellCheck
                    />
                    <WritingTools
                      key={`${meeting.id}:notes`}
                      meeting={meeting}
                      target="notes"
                      recipes={settings.recipes}
                      processingNote={
                        settings.provider === "local"
                          ? "Uses your local model."
                          : settings.hostedConsent
                            ? "Transcript and note context goes to OpenAI for this proposal."
                            : "Allow hosted transcript processing in Settings to continue."
                      }
                      allowed={
                        settings.provider === "local" || settings.hostedConsent
                      }
                      flush={() => flushEdits(meeting.id)}
                      merge={merge}
                      notify={notify}
                      onTimestamp={jump}
                    />
                    <div className="notes-bottom">
                      <ShieldCheck size={13} />
                      <span>Your notes are saved locally as you write.</span>
                    </div>
                  </section>
                )}
                {tab === "transcript" && (
                  <section
                    className="transcript-panel"
                    role="tabpanel"
                    aria-label="Transcript"
                  >
                    <RetainedAudio
                      key={meeting.id}
                      meeting={meeting}
                      merge={merge}
                      notify={notify}
                      beforeOperation={() => flushEdits()}
                      onRecovered={(m) => {
                        choose(m.id);
                        setTab("transcript");
                        setRecoverRequested(null);
                      }}
                      onSeekReady={bindAudioSeek}
                      openRequested={recoverRequested === meeting.id}
                    />
                    <div className="transcript-toolbar">
                      <label className="transcript-search">
                        <Search size={14} />
                        <input
                          aria-label="Search this transcript"
                          value={transcriptQuery}
                          onChange={(e) => setTranscriptQuery(e.target.value)}
                          placeholder="Find in transcript…"
                        />
                        {transcriptQuery && (
                          <button
                            className="icon-button small-button"
                            onClick={() => setTranscriptQuery("")}
                            aria-label="Clear transcript search"
                          >
                            <X size={12} />
                          </button>
                        )}
                      </label>
                      {recording && (
                        <button
                          className={`follow-button${followLive ? " selected" : ""}`}
                          onClick={() => setFollowLive((v) => !v)}
                        >
                          <ArrowDown size={13} />
                          {followLive ? "Following live" : "Follow live"}
                        </button>
                      )}
                      <span className="transcript-count">
                        {visibleSegments.length} segments
                      </span>
                    </div>
                    {meeting.speakers.length > 0 && (
                      <div className="speaker-legend">
                        {meeting.speakers.map((s) => (
                          <button
                            key={s.id}
                            title="Edit speaker name"
                            onClick={() => {
                              setRenameSpeaker(s);
                              setSpeakerName(s.name);
                            }}
                          >
                            <span
                              className={`speaker-dot speaker-${s.color % 6}`}
                            />
                            {s.name}
                          </button>
                        ))}
                      </div>
                    )}
                    {visibleSegments.map((s) => {
                      const speaker = meeting.speakers.find(
                        (p) => p.id === s.speakerId,
                      ) ?? {
                        id: s.speakerId,
                        name:
                          s.speakerId === "you"
                            ? "You"
                            : s.speakerId.replace("speaker-", "Speaker "),
                        color: 1,
                      };
                      const bookmark = meeting.bookmarks.find(
                        (b) => Math.abs(b.time - s.start) < 1,
                      );
                      return (
                        <div
                          key={s.id}
                          id={`segment-${s.id}`}
                          className={`transcript-segment${highlight === s.id ? " highlighted" : ""}`}
                        >
                          <Avatar speaker={speaker} />
                          <div className="segment-body">
                            <div className="segment-heading">
                              <button
                                className="speaker-name"
                                onClick={() => {
                                  setRenameSpeaker(speaker);
                                  setSpeakerName(speaker.name);
                                }}
                              >
                                {speaker.name}
                              </button>
                              <span className="segment-source">
                                {s.source === "microphone"
                                  ? "You"
                                  : s.source === "import"
                                    ? "Imported audio"
                                    : "Computer audio"}
                              </span>
                              <button
                                className="segment-time"
                                onClick={() => {
                                  setHighlight(s.id);
                                  setFollowLive(false);
                                  audioSeek.current?.(s.start);
                                }}
                              >
                                {clock(s.start)}
                              </button>
                              <button
                                className={`segment-bookmark${bookmark ? " bookmarked" : ""}`}
                                aria-label={
                                  bookmark
                                    ? "Remove bookmark"
                                    : "Bookmark this moment"
                                }
                                title={
                                  bookmark
                                    ? "Remove bookmark"
                                    : "Bookmark this moment"
                                }
                                onClick={() =>
                                  void run("bookmark", async () => {
                                    const m = await call<Meeting>(
                                      bookmark
                                        ? "remove_bookmark"
                                        : "add_bookmark",
                                      bookmark
                                        ? {
                                            meetingId: meeting.id,
                                            id: bookmark.id,
                                          }
                                        : {
                                            meetingId: meeting.id,
                                            time: s.start,
                                            label: s.text.slice(0, 90),
                                          },
                                    );
                                    merge(m);
                                  })
                                }
                              >
                                <Bookmark size={13} />
                              </button>
                            </div>
                            <p>
                              <MarkedText
                                text={s.text}
                                query={transcriptQuery}
                              />
                            </p>
                            <TranscriptCorrection
                              key={JSON.stringify([meeting.id, s.id])}
                              meeting={meeting}
                              segment={s}
                              draft={
                                correctionDrafts[
                                  JSON.stringify([meeting.id, s.id])
                                ]
                              }
                              onDraft={(draft) =>
                                changeCorrection(meeting.id, s.id, draft)
                              }
                              merge={merge}
                              notify={notify}
                            />
                          </div>
                        </div>
                      );
                    })}
                    {!visibleSegments.length &&
                      (transcriptQuery ? (
                        <div className="empty-inner">
                          <Search size={24} />
                          <h3>No matching words.</h3>
                          <p>Try a name or a shorter phrase.</p>
                          <button
                            className="button secondary compact"
                            onClick={() => setTranscriptQuery("")}
                          >
                            Clear search
                          </button>
                        </div>
                      ) : (
                        <EmptyTranscript recording={!!recording} />
                      ))}
                    <div ref={transcriptEnd} />
                    {meeting.segments.length > 0 && (
                      <div className="transcript-end">
                        <span />
                        <ShieldCheck size={12} />
                        <span>
                          {active ? "Local transcription" : "End of transcript"}
                        </span>
                        <span />
                      </div>
                    )}
                  </section>
                )}
              </article>
            </div>
            <div className="document-status">
              <span>
                <ShieldCheck size={12} />
                {meeting.isExample
                  ? "Example · no audio recorded"
                  : "Saved on this device"}
              </span>
              <span>
                {meeting.bookmarks.length > 0 && (
                  <button onClick={() => jump(meeting.bookmarks[0].time)}>
                    <Bookmark size={11} />
                    {meeting.bookmarks.length}{" "}
                    {meeting.bookmarks.length === 1 ? "moment" : "moments"}
                  </button>
                )}
                {settings.provider === "local"
                  ? "Local intelligence"
                  : settings.provider === "chatgpt"
                    ? "ChatGPT intelligence"
                    : "OpenAI intelligence"}
              </span>
            </div>
          </>
        )}
      </main>
      {view === "meeting" && assistantOpen && (
        <aside className="assistant-pane" aria-label="Meeting assistant">
          <header className="assistant-heading">
            <div>
              <span className="assistant-symbol">
                <Sparkles size={16} />
              </span>
              <h2>Ask Tuck</h2>
            </div>
            <button
              className="icon-button"
              title="Hide assistant"
              aria-label="Hide assistant"
              onClick={() => {
                setAssistantOpen(false);
                setPane("document");
              }}
            >
              <X size={16} />
            </button>
          </header>
          <div className="assistant-context">
            <span className={recording ? "live-context-dot" : "context-dot"} />
            {recording
              ? "Following this conversation"
              : meeting
                ? "This conversation in context"
                : "A thinking partner for your meetings"}
            {meeting && (
              <span className="context-count">{meeting.segments.length}</span>
            )}
          </div>
          <div className="assistant-messages">
            {(!meeting || !meeting.messages.length) &&
              !(stream && meeting && stream.meetingId === meeting.id) && (
                <div className="assistant-welcome">
                  <div className="assistant-art">
                    <MessageSquare size={26} strokeWidth={1.4} />
                    <Sparkles size={15} />
                  </div>
                  <h3>Keep your train of thought.</h3>
                  <p>
                    Ask a question without leaving the conversation. Tuck brings
                    the transcript along.
                  </p>
                  <div className="suggested-prompts">
                    {[
                      {
                        label: "What did I miss?",
                        prompt:
                          "Catch me up on the key points of this conversation so far.",
                        Icon: Clock,
                      },
                      {
                        label: "What should I ask next?",
                        prompt:
                          "Based on the conversation so far, suggest three useful questions I could ask next.",
                        Icon: MessageSquare,
                      },
                      {
                        label: "Pull out the next steps",
                        prompt:
                          "List the next steps agreed so far, with owners and timestamp citations. Do not invent missing owners.",
                        Icon: Check,
                      },
                    ].map(({ label, prompt: question, Icon }) => (
                      <button
                        key={label}
                        disabled={
                          !meeting || !meeting.segments.length || !!chatBusy
                        }
                        onClick={() => void ask(question)}
                      >
                        <Icon size={15} />
                        <span>{label}</span>
                        <ArrowUpRightIcon />
                      </button>
                    ))}
                  </div>
                  {meeting?.isExample && (
                    <p className="assistant-example-hint">
                      This example shows the workspace. Connect AI in the
                      desktop app to ask about a real conversation.
                    </p>
                  )}
                </div>
              )}
            {meeting?.messages.map((m) => (
              <div className={`chat-message ${m.role}`} key={m.id}>
                {m.role === "assistant" && (
                  <div className="assistant-message-label">
                    <Sparkles size={13} />
                    <span>Tuck</span>
                  </div>
                )}
                <Markdown
                  text={m.content}
                  onTimestamp={jump}
                  onSource={openSource}
                />
              </div>
            ))}
            {stream && meeting && stream.meetingId === meeting.id && (
              <div
                className="chat-message assistant streaming"
                aria-live="polite"
              >
                <div className="assistant-message-label">
                  <Sparkles size={13} />
                  <span>Tuck</span>
                </div>
                {stream?.text ? (
                  <Markdown text={stream.text} onTimestamp={jump} />
                ) : (
                  <div className="thinking-state">
                    <span />
                    <span />
                    <span />
                    <small>Thinking with your context…</small>
                  </div>
                )}
              </div>
            )}
            <div ref={messageEnd} />
          </div>
          <div className="assistant-composer-area">
            {settings.provider !== "local" && (
              <div className="assistant-data-note">
                <ShieldCheck size={11} />
                <span>
                  {settings.hostedConsent
                    ? "Transcript context goes to OpenAI."
                    : "Hosted AI needs permission in Settings."}
                </span>
              </div>
            )}
            <form
              className="assistant-composer"
              onSubmit={(e) => {
                e.preventDefault();
                void ask();
              }}
            >
              <textarea
                ref={promptRef}
                aria-label="Ask Tuck about this conversation"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void ask();
                  }
                }}
                placeholder={
                  meeting
                    ? "Ask about this conversation…"
                    : "Choose a conversation first…"
                }
                disabled={!meeting}
                rows={2}
              />
              <div>
                <button
                  type="button"
                  className="composer-context"
                  title="Select the AI provider in Settings"
                  onClick={() => {
                    setView("settings");
                    setPane("document");
                  }}
                >
                  <span className="context-dot" />
                  {settings.provider === "local"
                    ? "Local model"
                    : settings.provider === "chatgpt"
                      ? "ChatGPT"
                      : "OpenAI"}
                  <ChevronDown size={11} />
                </button>
                <button
                  className="send-button"
                  type="submit"
                  aria-label="Send question"
                  disabled={
                    !meeting ||
                    !prompt.trim() ||
                    !!chatBusy ||
                    (settings.provider !== "local" && !settings.hostedConsent)
                  }
                >
                  {chatBusy === meeting?.id ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <ArrowRight size={17} />
                  )}
                </button>
              </div>
            </form>
            <p className="assistant-footnote">
              A little help. Your judgment, always.
            </p>
          </div>
        </aside>
      )}
      <nav className="mobile-navigation" aria-label="Workspace panes">
        <button
          aria-current={pane === "library" ? "page" : undefined}
          onClick={() => setPane("library")}
        >
          <Library size={18} />
          Library
        </button>
        <button
          aria-current={pane === "document" ? "page" : undefined}
          onClick={() => setPane("document")}
        >
          <FileText size={18} />
          {view === "settings" ? "Settings" : "Conversation"}
        </button>
        <button
          aria-current={pane === "assistant" ? "page" : undefined}
          onClick={() => {
            setView("meeting");
            setAssistantOpen(true);
            setPane("assistant");
          }}
        >
          <Sparkles size={18} />
          Ask Tuck
        </button>
      </nav>
      {toast && (
        <div
          className={`toast${toast.error ? " error" : ""}`}
          role={toast.error ? "alert" : "status"}
        >
          <span>
            {toast.error ? <ShieldCheck size={17} /> : <Check size={17} />}
          </span>
          <p>{toast.text}</p>
          <button
            className="icon-button"
            onClick={() => setToast(null)}
            aria-label="Dismiss notification"
          >
            <X size={15} />
          </button>
        </div>
      )}
      <ConfirmDialog
        open={!!deleteId}
        title="Delete this conversation?"
        description="The transcript, notes, and retained audio will be removed from this device. Exports and any published Notion page are kept separately. This cannot be undone."
        onConfirm={remove}
        onClose={() => setDeleteId(null)}
      />
      {renameSpeaker && (
        <SpeakerDialog
          speaker={renameSpeaker}
          name={speakerName}
          setName={setSpeakerName}
          onClose={() => setRenameSpeaker(null)}
          onSave={() => {
            if (!meeting || !speakerName.trim()) return;
            void run("speaker", async () => {
              const m = await call<Meeting>("rename_speaker", {
                meetingId: meeting.id,
                speakerId: renameSpeaker.id,
                name: speakerName.trim(),
              });
              merge(m);
              setRenameSpeaker(null);
            });
          }}
        />
      )}
    </div>
  );
}
function SpeakerDialog({
  speaker,
  name,
  setName,
  onClose,
  onSave,
}: {
  speaker: Speaker;
  name: string;
  setName: (name: string) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    ref.current?.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      className="speaker-dialog"
      onCancel={onClose}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          onSave();
        }}
      >
        <div className="dialog-heading">
          <h2>Who's speaking?</h2>
          <button
            type="button"
            className="icon-button"
            aria-label="Close speaker editor"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <p>
          Names apply to this conversation. Speaker separation is provisional;
          you can correct it here.
        </p>
        <div className="speaker-input">
          <Avatar speaker={speaker} />
          <input
            aria-label="Speaker name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            maxLength={80}
          />
        </div>
        <div className="dialog-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="button primary"
            disabled={!name.trim()}
          >
            Save name
          </button>
        </div>
      </form>
    </dialog>
  );
}
function tabNavigation(e: KeyboardEvent<HTMLDivElement>) {
  if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(e.key)) return;
  const tabs = Array.from(
    e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=tab]"),
  );
  const at = tabs.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    e.key === "Home"
      ? 0
      : e.key === "End"
        ? tabs.length - 1
        : (at + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
  e.preventDefault();
  tabs[next]?.focus();
  tabs[next]?.click();
}
function HardDriveIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="5" width="18" height="14" rx="3" />
      <path d="M3 13h18M7 16h.01M11 16h.01" />
    </svg>
  );
}
function CpuIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <path d="M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4" />
    </svg>
  );
}
function NotionIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="2.5" y="2.5" width="15" height="15" rx="2" />
      <path d="M6 14V6l8 8V6M5 6h3M12 6h3M5 14h3" />
    </svg>
  );
}
function ArrowUpRightIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M5 15 15 5M5 5h10v10" />
    </svg>
  );
}
