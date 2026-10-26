import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  LoaderCircle,
  Search,
  Send,
  ShieldCheck,
} from "lucide-react";
import { call, subscribe } from "../lib/bridge";
import type {
  LibraryOverview,
  Meeting,
  MeetingPage,
  Message,
  SearchHit,
  Settings,
  TaskPage,
} from "../lib/types";
import { Markdown } from "./Markdown";
import { cursorStale, libraryScope } from "../lib/library";
import { clock } from "../lib/format";
export interface LibraryScope {
  folder: string;
  tag: string;
  person: string;
}
export function ScopeFilters({
  library,
  scope,
  onChange,
}: {
  library: LibraryOverview;
  scope: LibraryScope;
  onChange: (s: LibraryScope) => void;
}) {
  return (
    <>
    <details className="scope-filters" open={Object.values(scope).some(Boolean) || undefined}>
      <summary>
        Filter by folder, tag or person
        {Object.values(scope).some(Boolean) && <span className="status-dot" />}
      </summary>
      <div>
        {(
          [
            ["folder", "Folder", "folders"],
            ["tag", "Tag", "tags"],
            ["person", "Person", "people"],
          ] as const
        ).map(([key, label, field]) => (
          <label key={key}>
            {label}
            {library.facetsTruncated[field] ? (
              <>
                <input
                  aria-label={`Filter by ${label.toLowerCase()}`}
                  key={`${key}-${scope[key]}`}
                  defaultValue={scope[key]}
                  list={`library-${field}`}
                  placeholder={`Any ${label.toLowerCase()}`}
                  onBlur={(e) =>
                    onChange({ ...scope, [key]: e.target.value.trim() })
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                  }}
                />
                <datalist id={`library-${field}`}>
                  {library[field].map((v) => (
                    <option key={v} value={v} />
                  ))}
                </datalist>
              </>
            ) : (
              <select
                aria-label={`Filter by ${label.toLowerCase()}`}
                value={scope[key]}
                onChange={(e) => onChange({ ...scope, [key]: e.target.value })}
              >
                <option value="">
                  All {key === "person" ? "people" : `${label.toLowerCase()}s`}
                </option>
                {[
                  ...new Set([
                    ...library[field],
                    ...(scope[key] ? [scope[key]] : []),
                  ]),
                ].map((v) => (
                  <option key={v}>{v}</option>
                ))}
              </select>
            )}
          </label>
        ))}
        {Object.values(library.facetsTruncated).some(Boolean) && (
          <p className="quiet-note">
            Showing the first 200 choices. Type an exact name to filter beyond
            these suggestions.
          </p>
        )}
        {Object.values(scope).some(Boolean) && (
          <button
            className="text-link"
            onClick={() => onChange({ folder: "", tag: "", person: "" })}
          >
            Clear filters
          </button>
        )}
      </div>
    </details>
    {Object.values(scope).some(Boolean) && <div className="scope-chips" role="group" aria-label="Active filters">{(Object.keys(scope) as (keyof LibraryScope)[]).filter(key => scope[key]).map(key => <button key={key} aria-label={`Clear ${key} filter: ${scope[key]}`} onClick={() => onChange({...scope,[key]:""})}>{key}: {scope[key]} ×</button>)}</div>}
    </>
  );
}
export default function LibraryDesk({
  mode,
  focusRequest = 0,
  onScopeChange,
  library,
  revision,
  settings,
  scope,
  onSource,
  merge,
  notify,
}: {
  mode: "library" | "tasks";
  focusRequest?: number;
  onScopeChange: (scope: LibraryScope) => void;
  library: LibraryOverview;
  revision: number;
  settings: Settings;
  scope: LibraryScope;
  onSource: (url: string) => void;
  merge: (m: Meeting) => void;
  notify: (text: string, error?: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const queryRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (mode === "library") queryRef.current?.focus({preventScroll:true}); }, [mode, focusRequest]);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searched, setSearched] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [busy, setBusy] = useState(false);
  const [stream, setStream] = useState("");
  const [includeDone, setDone] = useState(false);
  const params = libraryScope(scope);
  const scopeKey = JSON.stringify(params);
  const [scopeCount, setScopeCount] = useState<number | null>(null);
  const [taskPage, setTaskPage] = useState<TaskPage>({
    tasks: [],
    nextCursor: null,
    total: 0,
  });
  const [tasksLoading, setTasksLoading] = useState(false);
  const [tasksError, setTasksError] = useState("");
  const [taskBusy, setTaskBusy] = useState<string | null>(null);
  const taskRequest = useRef(0);
  const pageRef = useRef(taskPage);
  pageRef.current = taskPage;
  const tasksPending = useRef(false);
  const loadTasks = useCallback(
    async (more = false) => {
      if (more && (tasksPending.current || !pageRef.current.nextCursor)) return;
      const token = ++taskRequest.current;
      tasksPending.current = true;
      setTasksLoading(true);
      setTasksError("");
      try {
        const page = await call<TaskPage>("list_task_page", {
          scope: JSON.parse(scopeKey),
          includeDone,
          limit: 50,
          ...(more ? { cursor: pageRef.current.nextCursor } : {}),
        });
        if (token !== taskRequest.current) return;
        setTaskPage((prev) =>
          more
            ? {
                ...page,
                tasks: [
                  ...prev.tasks,
                  ...page.tasks.filter(
                    (t) =>
                      !prev.tasks.some(
                        (p) => p.id === t.id && p.meetingId === t.meetingId,
                      ),
                  ),
                ],
              }
            : page,
        );
      } catch (e) {
        if (token !== taskRequest.current) return;
        if (more && cursorStale(e)) {
          tasksPending.current = false;
          await loadTasks(false);
          return;
        }
        setTasksError(e instanceof Error ? e.message : String(e));
      } finally {
        if (token === taskRequest.current) {
          tasksPending.current = false;
          setTasksLoading(false);
        }
      }
    },
    [scopeKey, includeDone],
  );
  useEffect(() => {
    if (mode !== "tasks") return;
    void loadTasks();
    return () => {
      taskRequest.current++;
    };
  }, [mode, loadTasks, revision]);
  useEffect(() => {
    if (mode !== "library") return;
    let current = true;
    if (!Object.values(scope).some(Boolean)) {
      setScopeCount(library.meetingCount);
      return;
    }
    setScopeCount(null);
    void call<MeetingPage>("list_meetings", {
      scope: JSON.parse(scopeKey),
      limit: 1,
    })
      .then((page) => {
        if (current) setScopeCount(page.total);
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [mode, scopeKey, library.meetingCount, revision]);
  useEffect(() => {
    setHits([]);
    setSearched(false);
  }, [scopeKey]);
  const execute = async (kind: "search" | "ask") => {
    if (!query.trim() || busy) return;
    setBusy(true);
    let unlisten: (() => void) | undefined;
    const requestId = crypto.randomUUID();
    try {
      if (kind === "search") {
        setSearched(true);
        setHits(
          await call<SearchHit[]>("search_library", {
            query,
            ...params,
            limit: 24,
          }),
        );
      } else {
        unlisten = await subscribe((e) => {
          if (
            e.event === "library_chat_delta" &&
            e.payload.requestId === requestId
          )
            setStream((v) => v + String(e.payload.delta));
        });
        const reply = await call<Message>("chat_library", {
          prompt: query,
          requestId,
          ...params,
        });
        setMessages((v) => [
          ...v,
          {
            id: requestId,
            role: "user",
            content: query,
            createdAt: new Date().toISOString(),
          },
          reply,
        ]);
        setQuery("");
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
    } finally {
      unlisten?.();
      setBusy(false);
      setStream("");
    }
  };
  if (mode === "tasks")
    return (
      <article className="library-desk">
        <h1>Tasks</h1>
        <p className="section-description">
          Commitments across your conversations, with their context close by.
        </p>
        <ScopeFilters library={library} scope={scope} onChange={onScopeChange} />
        <label className="inline-check">
          <input
            type="checkbox"
            checked={includeDone}
            onChange={(e) => setDone(e.target.checked)}
          />
          Show completed
        </label>
        {taskPage.tasks.map((t) => (
          <div
            className={`inbox-task${t.done ? " done" : ""}`}
            key={`${t.meetingId}:${t.id}`}
          >
            <input
              type="checkbox"
              aria-label={`Complete ${t.title}`}
              checked={t.done}
              disabled={tasksLoading || taskBusy === `${t.meetingId}:${t.id}`}
              onChange={async () => {
                setTaskBusy(`${t.meetingId}:${t.id}`);
                try {
                  merge(
                    await call<Meeting>("update_task", {
                      meetingId: t.meetingId,
                      id: t.id,
                      changes: { done: !t.done },
                    }),
                  );
                } catch (e) {
                  notify(String(e), true);
                } finally {
                  setTaskBusy(null);
                }
              }}
            />
            <div>
              <strong>{t.title}</strong>
              <span>
                {[t.owner, t.dueDate && `Due ${t.dueDate}`]
                  .filter(Boolean)
                  .join(" · ") || "No owner or due date"}
              </span>
              <button
                className="text-link"
                onClick={() =>
                  onSource(`tuck://meeting/${t.meetingId}?source=tasks`)
                }
              >
                {t.meetingTitle}
                <ArrowRight size={12} />
              </button>
            </div>
          </div>
        ))}
        {tasksLoading && (
          <p className="loading-note" role="status">
            <LoaderCircle size={14} className="spin" />
            Loading tasks…
          </p>
        )}
        {tasksError && (
          <div className="inline-retry" role="alert">
            <p>{tasksError}</p>
            <button className="text-link" onClick={() => void loadTasks()}>
              Try again
            </button>
          </div>
        )}
        {taskPage.nextCursor && (
          <button
            className="button secondary compact load-more"
            disabled={tasksLoading}
            onClick={() => void loadTasks(true)}
          >
            Load more tasks{" "}
            <span>
              {taskPage.tasks.length} of {taskPage.total}
            </span>
          </button>
        )}
        {!taskPage.tasks.length && !tasksLoading && !tasksError && (
          <div className="desk-empty">
            <Check size={27} />
            <h2>{includeDone ? "No tasks here yet." : "You're up to date."}</h2>
            <p>
              Add next steps from a meeting's Tasks tab. Folder, tag and people
              filters also apply here.
            </p>
          </div>
        )}
      </article>
    );
  return (
    <article className="library-desk">
      <h1>Search library</h1>
      <p className="section-description">
        Find matching passages with keyword search, or ask Tuck a question across your saved meetings and voice notes.
      </p>
      <ScopeFilters library={library} scope={scope} onChange={onScopeChange} />
      <div className="desk-scope">
        {scopeCount === null
          ? "Checking scope…"
          : `${scopeCount} ${scopeCount === 1 ? "conversation" : "conversations"}`}
        {Object.values(scope).some(Boolean)
          ? " in your current filters"
          : " in your library"}
      </div>
      <form
        className="library-question"
        onSubmit={(e) => {
          e.preventDefault();
          void execute("search");
        }}
      >
        <label className="visually-hidden" htmlFor="library-question">
          Search or ask your library
        </label>
        <textarea
          ref={queryRef}
          id="library-question"
          rows={3}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="What did we decide about the launch?"
        />
        <div>
          <button
            className="button secondary compact"
            disabled={busy || !query.trim()}
          >
            <Search size={14} />
            Search passages
          </button>
          <button
            type="button"
            className="button primary compact"
            disabled={
              busy ||
              !query.trim() ||
              (settings.provider !== "local" && !settings.hostedConsent)
            }
            onClick={() => void execute("ask")}
          >
            {busy ? (
              <LoaderCircle size={14} className="spin" />
            ) : (
              <Send size={14} />
            )}
            Ask Tuck
          </button>
        </div>
      </form>
      <p className="quiet-note">
        <ShieldCheck size={12} />
        {settings.provider === "local"
          ? "Answers use your local model and a bounded selection of matching passages."
          : settings.hostedConsent
            ? "Matching transcript and note passages go to OpenAI when you ask."
            : "Allow hosted transcript processing in Settings to ask a question."}{" "}
        Keyword search runs locally.
      </p>
      {hits.length > 0 && (
        <section className="library-results">
          <h2>Matching passages</h2>
          <p className="quiet-note">
            Keyword retrieval. These passages match words in your search.
          </p>
          {hits.map((h, i) => (
            <button
              className="source-result"
              key={`${h.meetingId}-${h.segmentId || h.source}-${i}`}
              onClick={() =>
                onSource(
                  h.url ||
                    `tuck://meeting/${h.meetingId}?source=${h.source}&start=${h.start || 0}`,
                )
              }
            >
              <span>
                {h.meetingTitle}
                <small>
                  {h.start !== undefined ? clock(h.start) : h.source}
                </small>
              </span>
              <p>{h.text}</p>
              <span className="text-link">
                Open source
                <ArrowRight size={12} />
              </span>
            </button>
          ))}
        </section>
      )}
      {messages.map((m) => (
        <div className={`library-message ${m.role}`} key={m.id}>
          <strong>{m.role === "user" ? "You" : "Tuck"}</strong>
          <Markdown
            text={m.content}
            onSource={(url) => {
              const allowed =
                m.role === "assistant" &&
                m.citations?.some((c) => c.url === url);
              if (allowed) onSource(url);
              else
                notify(
                  "This link is not one of the retrieved sources for this answer.",
                  true,
                );
            }}
          />
          {m.citations?.map((c, i) => (
            <button
              className="citation source-chip"
              key={i}
              onClick={() =>
                onSource(
                  c.url ||
                    `tuck://meeting/${c.meetingId}?start=${c.start || 0}&source=${c.source}`,
                )
              }
            >
              {c.meetingTitle}
              {c.start !== undefined ? ` · ${clock(c.start)}` : ""}
            </button>
          ))}
        </div>
      ))}
      {busy && stream && <Markdown text={stream} />}
      {searched && !hits.length && !busy && (
        <p className="quiet-note" role="status">
          No matching passages. Try a shorter phrase or clear your library
          filters.
        </p>
      )}
      {!searched && !hits.length && !messages.length && (
        <div className="desk-empty">
          <Search size={27} />
          <h2>Start with something you remember.</h2>
          <p>
            Search a name or phrase to see its source. Ask a question to compare
            decisions, commitments or themes across meetings.
          </p>
        </div>
      )}
    </article>
  );
}
