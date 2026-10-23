import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import type { MeetingAgent, Settings } from "../lib/types";

const DRAFT_KEY = "tuck.meeting-agent-draft";
let sessionDraft: MeetingAgent | null = null;
function readDraft(): MeetingAgent | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(DRAFT_KEY) || "null");
    if (value && typeof value.id === "string" && typeof value.name === "string" && typeof value.instructions === "string" && value.name.length <= 80 && value.instructions.length <= 4000 && (value.pace === "quiet" || value.pace === "active")) return value;
  } catch { /* The in-memory session copy still preserves navigation drafts. */ }
  return sessionDraft;
}
function rememberDraft(draft: MeetingAgent | null) {
  sessionDraft = draft;
  try {
    if (draft) sessionStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
    else sessionStorage.removeItem(DRAFT_KEY);
  } catch { /* Some native webviews restrict sessionStorage; navigation remains safe. */ }
}

export default function MeetingAgents({ agents, onUpdate }: {
  agents: MeetingAgent[];
  onUpdate: (changes: Partial<Settings>) => Promise<void>;
}) {
  const [draft, setDraft] = useState<MeetingAgent | null>(readDraft);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => { rememberDraft(draft); }, [draft]);
  async function save(next: MeetingAgent[]) {
    setBusy(true); setError(""); setNotice("");
    try { await onUpdate({ meetingAgents: next }); rememberDraft(null); setDraft(null); setRemoving(false); setNotice("Meeting agents saved. Choose them when you turn on coaching for a call."); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  }
  function cancel() {
    rememberDraft(null); setDraft(null); setRemoving(false); setError("");
  }
  return <section className="meeting-agents" aria-label="Meeting agents">
    <h2>Meeting agents</h2>
    <p className="section-description">Choose what Tuck watches for during a call. Agents share your selected AI provider and only run when you turn them on for that call.</p>
    <ul className="meeting-agent-list">{agents.map((agent) => <li key={agent.id}><div><strong>{agent.name}</strong><span>{agent.pace === "quiet" ? "Quiet · only useful moments" : "Active · more frequent guidance"}</span></div><button className="button secondary compact" disabled={busy || !!draft} onClick={() => { setDraft({ ...agent }); setRemoving(false); setError(""); setNotice(""); }} aria-label={`Edit ${agent.name}`}>Edit</button></li>)}</ul>
    {!draft && <button className="button secondary compact" disabled={busy || agents.length >= 8} onClick={() => { setDraft({ id: crypto.randomUUID(), name: "", instructions: "", pace: "quiet" }); setRemoving(false); setNotice(""); }}><Plus size={14} />Add agent</button>}
    {agents.length >= 8 && !draft && <p className="quiet-note">You have 8 agents. Edit an existing agent to change its focus.</p>}
    {draft && <form className="writing-form meeting-agent-form" onSubmit={(event) => { event.preventDefault(); void save([...agents.filter((a) => a.id !== draft.id), { ...draft, name: draft.name.trim(), instructions: draft.instructions.trim() }]); }}>
      <p className="quiet-note">Your draft stays here if you leave this view. Save or cancel it before editing another agent.</p>
      <label>Name<input aria-label="Agent name" maxLength={80} required disabled={busy} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
      <label>What should this agent watch for?<textarea aria-label="Agent instructions" placeholder="Track unanswered pricing questions and suggest a helpful follow-up." maxLength={4000} rows={4} required disabled={busy} value={draft.instructions} onChange={(e) => setDraft({ ...draft, instructions: e.target.value })} /></label>
      <label>How often should it help?<select aria-label="Agent pace" disabled={busy} value={draft.pace} onChange={(e) => setDraft({ ...draft, pace: e.target.value as MeetingAgent["pace"] })}><option value="quiet">Quiet · only useful moments</option><option value="active">Active · more frequent guidance</option></select></label>
      <div className="proposal-actions"><button className="button primary compact" disabled={busy || removing || !draft.name.trim() || !draft.instructions.trim()}>{busy && !removing ? "Saving…" : "Save agent"}</button><button type="button" className="button secondary compact" disabled={busy} onClick={cancel}>Cancel</button>{agents.some((a) => a.id === draft.id) && !removing && <button type="button" className="button secondary compact" disabled={busy} onClick={() => setRemoving(true)}>Remove agent</button>}</div>
      {removing && <div className="meeting-agent-remove" role="group" aria-label="Confirm agent removal"><p>Remove {agents.find((a) => a.id === draft.id)?.name || "this agent"}? Its saved instructions will be deleted.</p><div className="proposal-actions"><button type="button" className="button secondary compact" disabled={busy} onClick={() => { setRemoving(false); setError(""); }}>Keep agent</button><button type="button" className="button danger compact" disabled={busy} onClick={() => void save(agents.filter((a) => a.id !== draft.id))}>{busy ? "Removing…" : "Confirm removal"}</button></div></div>}
      <p className="quiet-note">Agents suggest; you decide. They cannot send messages, browse the web or take actions for you.</p>
    </form>}
    {error && <p className="meeting-agent-error" role="alert">{error} Your edits are still here; try saving again.</p>}
    {notice && <p className="quiet-note" role="status">{notice}</p>}
  </section>;
}
