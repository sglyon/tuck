import { useEffect, useState } from "react";
import {
  ArrowRight,
  Check,
  LoaderCircle,
  Search,
  Send,
  ShieldCheck,
} from "lucide-react";
import { call, subscribe } from "../lib/bridge";
import type { Meeting, Message, SearchHit, Settings } from "../lib/types";
import { Markdown } from "./Markdown";
import { clock } from "../lib/format";
export interface LibraryScope {
  folder: string;
  tag: string;
  person: string;
}
export function ScopeFilters({
  meetings,
  scope,
  onChange,
}: {
  meetings: Meeting[];
  scope: LibraryScope;
  onChange: (s: LibraryScope) => void;
}) {
  const values = (field: "folder" | "tags" | "people") =>
    [
      ...new Set(
        meetings.flatMap((m) =>
          field === "folder" ? (m.folder ? [m.folder] : []) : m[field] || [],
        ),
      ),
    ].sort();
  return (
    <details className="scope-filters">
      <summary>
        Filter by folder, tag or person
        {Object.values(scope).some(Boolean) && <span className="status-dot" />}
      </summary>
      <div>
        {(
          [
            ["folder", "Folder", "folder"],
            ["tag", "Tag", "tags"],
            ["person", "Person", "people"],
          ] as const
        ).map(([key, label, field]) => (
          <label key={key}>
            {label}
            <select
              aria-label={`Filter by ${label.toLowerCase()}`}
              value={scope[key]}
              onChange={(e) => onChange({ ...scope, [key]: e.target.value })}
            >
              <option value="">
                All {key === "person" ? "people" : `${label.toLowerCase()}s`}
              </option>
              {values(field).map((v) => (
                <option key={v}>{v}</option>
              ))}
            </select>
          </label>
        ))}
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
  );
}
export default function LibraryDesk({
  mode,
  meetings,
  settings,
  scope,
  onSource,
  merge,
  notify,
}: {
  mode: "library" | "tasks";
  meetings: Meeting[];
  settings: Settings;
  scope: LibraryScope;
  onSource: (url: string) => void;
  merge: (m: Meeting) => void;
  notify: (text: string, error?: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searched, setSearched] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [busy, setBusy] = useState(false);
  const [stream, setStream] = useState("");
  const [includeDone, setDone] = useState(false);
  const params = {
    folder: scope.folder || undefined,
    tags: scope.tag ? [scope.tag] : undefined,
    people: scope.person ? [scope.person] : undefined,
  };
  useEffect(() => {
    setHits([]);
    setSearched(false);
  }, [scope.folder, scope.tag, scope.person]);
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
  const scoped = meetings.filter(
    (m) =>
      (!scope.folder || m.folder === scope.folder) &&
      (!scope.tag || m.tags?.includes(scope.tag)) &&
      (!scope.person || m.people?.includes(scope.person)),
  );
  const tasks = scoped
    .flatMap((m) =>
      (m.tasks || [])
        .filter((t) => includeDone || !t.done)
        .map((t) => ({ ...t, meeting: m })),
    )
    .sort((a, b) => (a.dueDate || "9999").localeCompare(b.dueDate || "9999"));
  if (mode === "tasks")
    return (
      <article className="library-desk">
        <h1>Tasks</h1>
        <p className="section-description">
          Commitments across your conversations, with their context close by.
        </p>
        <label className="inline-check">
          <input
            type="checkbox"
            checked={includeDone}
            onChange={(e) => setDone(e.target.checked)}
          />
          Show completed
        </label>
        {tasks.map((t) => (
          <div className={`inbox-task${t.done ? " done" : ""}`} key={t.id}>
            <input
              type="checkbox"
              aria-label={`Complete ${t.title}`}
              checked={t.done}
              onChange={async () => {
                try {
                  merge(
                    await call<Meeting>("update_task", {
                      meetingId: t.meeting.id,
                      id: t.id,
                      changes: { done: !t.done },
                    }),
                  );
                } catch (e) {
                  notify(String(e), true);
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
                  onSource(`tuck://meeting/${t.meeting.id}?source=tasks`)
                }
              >
                {t.meeting.title}
                <ArrowRight size={12} />
              </button>
            </div>
          </div>
        ))}
        {!tasks.length && (
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
      <h1>Ask your library</h1>
      <p className="section-description">
        Find a spoken detail, or connect the dots across your meetings.
      </p>
      <div className="desk-scope">
        {scoped.length} {scoped.length === 1 ? "conversation" : "conversations"}
        {Object.values(scope).some(Boolean)
          ? " in your current filters"
          : " in your library"}
        {scoped.some((m) => m.isExample) && " · includes illustrative examples"}
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
      {searched && !hits.length && (
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
