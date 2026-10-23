import { useEffect, useState } from "react";
import {
  Check,
  ChevronRight,
  History,
  LoaderCircle,
  Plus,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { call } from "../lib/bridge";
import type {
  Meeting,
  MeetingChanges,
  Proposal,
  Recipe,
  Revision,
} from "../lib/types";
import { Markdown } from "./Markdown";

type Shared = {
  meeting: Meeting;
  merge: (m: Meeting) => void;
  notify: (text: string, error?: boolean) => void;
};
export function Organization({
  meeting,
  update,
}: {
  meeting: Meeting;
  update: (changes: MeetingChanges) => Promise<void>;
}) {
  return (
    <details className="meeting-organization">
      <summary>
        <ChevronRight size={13} />
        {meeting.folder || "Organize meeting"}
        {!!meeting.tags?.length && <span>{meeting.tags.join(", ")}</span>}
      </summary>
      <div className="organization-fields">
        <label>
          Folder
          <input
            key={`${meeting.id}-folder`}
            aria-label="Meeting folder"
            defaultValue={meeting.folder || ""}
            placeholder="e.g. Acme"
            onBlur={(e) =>
              void update({ folder: e.target.value.trim() }).catch(() => {})
            }
          />
        </label>
        <label>
          Tags
          <input
            key={`${meeting.id}-tags`}
            aria-label="Meeting tags"
            defaultValue={(meeting.tags || []).join(", ")}
            placeholder="e.g. sales, renewal"
            onBlur={(e) =>
              void update({
                tags: e.target.value
                  .split(",")
                  .map((v) => v.trim())
                  .filter(Boolean),
              }).catch(() => {})
            }
          />
        </label>
        <label>
          People
          <input
            key={`${meeting.id}-people`}
            aria-label="Meeting people"
            defaultValue={(meeting.people || []).join(", ")}
            placeholder="Names, separated by commas"
            onBlur={(e) =>
              void update({
                people: e.target.value
                  .split(",")
                  .map((v) => v.trim())
                  .filter(Boolean),
              }).catch(() => {})
            }
          />
        </label>
      </div>
    </details>
  );
}
export function Tasks({ meeting, merge, notify }: Shared) {
  const [title, setTitle] = useState("");
  const [owner, setOwner] = useState("");
  const [dueDate, setDue] = useState("");
  const [busy, setBusy] = useState(false);
  const change = async (method: string, params: object) => {
    setBusy(true);
    try {
      merge(await call<Meeting>(method, { meetingId: meeting.id, ...params }));
      return true;
    } catch (e) {
      notify(String(e), true);
      return false;
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="task-panel" aria-label="Meeting tasks">
      <div className="section-heading">
        <Check size={18} />
        <h2>Next steps</h2>
      </div>
      <p className="section-description">
        A place for commitments you want to follow through.
      </p>
      {(meeting.tasks || []).map((t) => (
        <div className={`task-row${t.done ? " done" : ""}`} key={t.id}>
          <input
            type="checkbox"
            aria-label={`Complete ${t.title}`}
            checked={t.done}
            disabled={busy}
            onChange={() =>
              void change("update_task", {
                id: t.id,
                changes: { done: !t.done },
              })
            }
          />
          <div>
            <textarea
              rows={1}
              ref={(node) => {
                if (node) {
                  node.style.height = "auto";
                  node.style.height = `${node.scrollHeight}px`;
                }
              }}
              onInput={(e) => {
                e.currentTarget.style.height = "auto";
                e.currentTarget.style.height = `${e.currentTarget.scrollHeight}px`;
              }}
              className="task-title"
              aria-label={`Task title: ${t.title}`}
              defaultValue={t.title}
              onBlur={(e) => {
                if (e.target.value.trim() !== t.title)
                  void change("update_task", {
                    id: t.id,
                    changes: { title: e.target.value.trim() },
                  });
              }}
            />
            <div className="task-details">
              <input
                aria-label={`Owner for ${t.title}`}
                placeholder="Owner"
                defaultValue={t.owner}
                onBlur={(e) =>
                  void change("update_task", {
                    id: t.id,
                    changes: { owner: e.target.value.trim() },
                  })
                }
              />
              <input
                type="date"
                aria-label={`Due date for ${t.title}`}
                defaultValue={t.dueDate}
                onChange={(e) =>
                  void change("update_task", {
                    id: t.id,
                    changes: { dueDate: e.target.value },
                  })
                }
              />
            </div>
          </div>
          <button
            className="icon-button"
            aria-label={`Delete task ${t.title}`}
            disabled={busy}
            onClick={() => void change("delete_task", { id: t.id })}
          >
            <Trash2 size={14} />
          </button>
        </div>
      ))}
      <form
        className="task-add"
        onSubmit={async (e) => {
          e.preventDefault();
          if (await change("add_task", { title, owner, dueDate })) {
            setTitle("");
            setOwner("");
            setDue("");
          }
        }}
      >
        <input
          aria-label="New task"
          placeholder="Add a next step…"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={500}
        />
        <div>
          <input
            aria-label="New task owner"
            placeholder="Owner (optional)"
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
          />
          <input
            aria-label="New task due date"
            type="date"
            value={dueDate}
            onChange={(e) => setDue(e.target.value)}
          />
          <button
            className="button secondary compact"
            disabled={busy || !title.trim()}
          >
            <Plus size={14} />
            Add task
          </button>
        </div>
      </form>
      {!(meeting.tasks || []).length && (
        <p className="quiet-note">
          No tasks yet. Add a commitment as it comes up.
        </p>
      )}
    </section>
  );
}
export function WritingTools({
  meeting,
  target,
  recipes,
  processingNote,
  allowed,
  flush,
  merge,
  notify,
  onTimestamp,
}: Shared & {
  target: "notes" | "summary";
  recipes: Recipe[];
  processingNote: string;
  allowed: boolean;
  flush: () => Promise<boolean>;
  onTimestamp: (t: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const [instruction, setInstruction] = useState("");
  const [busy, setBusy] = useState(false);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [history, setHistory] = useState<Revision[] | null>(null);
  useEffect(() => {
    setProposal(null);
    setHistory(null);
    setOpen(false);
  }, [meeting.id, target]);
  const action = async (fn: () => Promise<void>) => {
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
    <div className="writing-tools">
      <div className="writing-actions">
        <button
          className="text-link"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
        >
          <Sparkles size={13} />
          Enhance {target === "notes" ? "notes" : "recap"}
        </button>
        <button
          className="text-link"
          disabled={busy}
          onClick={() =>
            void action(async () =>
              setHistory(
                history
                  ? null
                  : await call<Revision[]>("note_history", {
                      meetingId: meeting.id,
                    }),
              ),
            )
          }
        >
          <History size={13} />
          Revision history
        </button>
      </div>
      {open && (
        <div className="writing-request">
          <label>
            What would make this more useful?
            <textarea
              aria-label="Writing instruction"
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              placeholder="For example: make this a concise client follow-up with clear next steps."
              rows={2}
            />
          </label>
          {recipes.length > 0 && (
            <label>
              Saved recipe
              <select
                aria-label="Choose recipe"
                value=""
                onChange={(e) =>
                  setInstruction(
                    recipes.find((r) => r.id === e.target.value)?.prompt || "",
                  )
                }
              >
                <option value="">Choose a recipe…</option>
                {recipes.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <button
            className="button subtle compact"
            disabled={busy || !instruction.trim() || !allowed}
            onClick={() =>
              void action(async () => {
                if (!(await flush()))
                  throw new Error(
                    "Save your edits before asking for a proposal.",
                  );
                setProposal(
                  await call<Proposal>("propose_notes", {
                    meetingId: meeting.id,
                    target,
                    instruction,
                    requestId: crypto.randomUUID(),
                  }),
                );
              })
            }
          >
            {busy ? (
              <LoaderCircle size={14} className="spin" />
            ) : (
              <Sparkles size={14} />
            )}
            Preview changes
          </button>
          <p className="quiet-note">
            Review the proposal before replacing your text. {processingNote}
          </p>
        </div>
      )}
      {proposal && (
        <section className="proposal-preview" aria-label="Proposed changes">
          <div className="section-heading">
            <h3>Proposed {target === "notes" ? "notes" : "recap"}</h3>
            <button
              className="icon-button"
              aria-label="Reject proposal"
              onClick={() => setProposal(null)}
            >
              <X size={16} />
            </button>
          </div>
          <Markdown text={proposal.text} onTimestamp={onTimestamp} />
          <div className="proposal-actions">
            <button
              className="button primary compact"
              disabled={busy}
              onClick={() =>
                void action(async () => {
                  if (!(await flush()))
                    throw new Error("Save your latest edits first.");
                  merge(
                    await call<Meeting>("apply_proposal", {
                      meetingId: meeting.id,
                      ...proposal,
                    }),
                  );
                  setProposal(null);
                  notify(
                    "Changes accepted. Your previous text is in revision history.",
                  );
                })
              }
            >
              Accept changes
            </button>
            <button
              className="button secondary compact"
              onClick={() => setProposal(null)}
            >
              Keep my text
            </button>
          </div>
          <p className="quiet-note">
            If you edit the original text, generate a fresh proposal before
            accepting.
          </p>
        </section>
      )}
      {history && (
        <section className="revision-history" aria-label="Revision history">
          <h3>Previous versions</h3>
          <p className="quiet-note">
            Manual versions are kept periodically. Accepted AI changes also keep
            the exact previous text.
          </p>
          {history
            .filter((r) => r.target === target)
            .map((r) => (
              <details key={r.id}>
                <summary>{new Date(r.createdAt).toLocaleString()}</summary>
                <Markdown text={r.text} onTimestamp={onTimestamp} />
              </details>
            ))}
          {!history.some((r) => r.target === target) && (
            <p className="quiet-note">No previous versions yet.</p>
          )}
        </section>
      )}
    </div>
  );
}
