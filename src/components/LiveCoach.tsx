import { useEffect, useRef, useState } from "react";
import { Check, X } from "lucide-react";
import { call, subscribe } from "../lib/bridge";
import { clock } from "../lib/format";
import type { LiveCoachState, Meeting, Settings } from "../lib/types";

const off: LiveCoachState = { enabled: false, status: "off", selectedAgentIds: [], suggestions: [] };

/** The native worker owns scheduling; opening this pane never starts inference. */
export default function LiveCoach({ meeting, settings, onMeeting, onSource, onSettings }: {
  meeting: Meeting;
  settings: Settings;
  onMeeting: (meeting: Meeting) => void;
  onSource: (time: number) => void;
  onSettings: () => void;
}) {
  const [coach, setCoach] = useState<LiveCoachState>(off);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [announcement, setAnnouncement] = useState("");
  const knownSuggestions = useRef<Set<string> | null>(null);
  const agents = settings.meetingAgents || [];
  const [selected, setSelected] = useState<string[]>(() => {
    const first = agents.find((a) => a.pace === "quiet") || agents[0];
    return first ? [first.id] : [];
  });
  const alive = useRef(true);
  const revision = useRef(0);
  const retry = useRef<{ method: string; params: Record<string, unknown> } | null>(null);
  const live = meeting.status === "recording" || meeting.status === "paused";
  const provider = settings.provider === "local" ? "your local AI model" : settings.provider === "chatgpt" ? "ChatGPT" : "OpenAI";
  const hostedBlocked = settings.provider !== "local" && !settings.hostedConsent;

  useEffect(() => {
    alive.current = true;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const stamp = revision.current;
    void subscribe((event) => {
      if (event.event !== "coach_updated" || event.payload.meetingId !== meeting.id) return;
      const next = event.payload.coach as LiveCoachState | undefined;
      if (!next || disposed) return;
      revision.current++;
      setCoach(next);
      setLoading(false);
    }).then((stop) => { if (!disposed) unlisten = stop; else stop(); });
    void call<LiveCoachState>("get_live_coach", { meetingId: meeting.id })
      .then((next) => { if (!disposed && revision.current === stamp) { setCoach(next); if (next.selectedAgentIds.length) setSelected(next.selectedAgentIds); } })
      .catch((e) => { if (!disposed && live) { retry.current = { method: "get_live_coach", params: {} }; setError(String(e instanceof Error ? e.message : e)); } })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; alive.current = false; unlisten?.(); };
    // App keys this component by meeting ID. Capture lifecycle events update its status.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meeting.id]);

  useEffect(() => {
    if (loading) return;
    const added = knownSuggestions.current ? coach.suggestions.filter((item) => item.status === "open" && !knownSuggestions.current!.has(item.id)).length : 0;
    setAnnouncement(added ? `${added} new live coach ${added === 1 ? "suggestion" : "suggestions"}.` : "");
    knownSuggestions.current = new Set(coach.suggestions.map((item) => item.id));
  }, [coach.suggestions, loading]);

  async function action(method: string, params: Record<string, unknown> = {}) {
    if (busy) return;
    setBusy(method);
    setError("");
    setNotice("");
    retry.current = null;
    revision.current++;
    try {
      if (method === "save_coach_follow_up") {
        const result = await call<{ coach: LiveCoachState; meeting: Meeting }>(method, { meetingId: meeting.id, ...params });
        if (!alive.current) return;
        setCoach(result.coach);
        onMeeting(result.meeting);
        setNotice("Saved to this meeting’s Tasks. Review the owner and due date there.");
      } else {
        const result = await call<LiveCoachState>(method, { meetingId: meeting.id, ...params });
        if (alive.current) setCoach(result);
      }
    } catch (e) {
      if (alive.current) { retry.current = { method, params }; setError(e instanceof Error ? e.message : String(e)); }
    } finally {
      if (alive.current) setBusy("");
    }
  }

  const items = coach.suggestions.filter((s) => s.status === "open").slice(0, 3);
  const saved = coach.suggestions.filter((s) => s.status === "saved").length;
  if (!live && !items.length && !saved && !coach.enabled) return null;
  const ended = !live;
  const activeNames = coach.selectedAgentIds.map((id) => agents.find((agent) => agent.id === id)?.name).filter(Boolean).join(", ");
  const backgroundUnavailable = !error && coach.status === "unavailable" && !!coach.message;
  const status = loading ? "Checking live coach…" : ended ? "Call ended · coaching stopped" : meeting.status === "paused" ? "Paused with recording" : coach.status === "analyzing" ? "Reviewing recent conversation…" : coach.status === "unavailable" ? "Coaching needs attention" : coach.enabled ? "Listening for useful moments" : "Off for this call";
  return (
    <section className="live-coach" aria-label="Live coach">
      <p className="visually-hidden" role="status" aria-live="polite" aria-atomic="true">{announcement}</p>
      <div className="live-coach-heading">
        <div><h3>Live coach</h3><p aria-live="polite">{status}</p>{coach.enabled && activeNames && <p className="live-coach-active-agents">Agents: {activeNames}</p>}</div>
        {coach.enabled && !ended && <button className="button secondary compact" disabled={!!busy} onClick={() => void action("set_live_coach", { enabled: false })}>{busy === "set_live_coach" ? "Turning off…" : "Turn off"}</button>}
      </div>
      {!coach.enabled && !ended && <>
        <p className="live-coach-description">A useful question or follow-up, when the conversation calls for it.</p>
        <fieldset className="live-coach-agents"><legend>Choose agents for this call</legend>{agents.map((agent) => <label key={agent.id}><input type="checkbox" checked={selected.includes(agent.id)} disabled={!!busy || (!selected.includes(agent.id) && selected.length >= 3)} onChange={(e) => setSelected((ids) => e.target.checked ? [...ids, agent.id] : ids.filter((id) => id !== agent.id))} /><span>{agent.name}<small>{agent.pace === "quiet" ? "Quiet · only useful moments" : "Active · more frequent guidance"}</small></span></label>)}</fieldset>
        {selected.length >= 3 && <p className="live-coach-quiet">Up to 3 agents can help with one call.</p>}
        <p className="live-coach-privacy">{settings.provider === "local" ? "Uses your local AI model for this call only." : `Automatically sends recent transcript excerpts, selected agent instructions and earlier tips to ${provider} while enabled. This call only.`}</p>
        {hostedBlocked ? <button className="button secondary compact" onClick={onSettings}>Allow hosted AI in Settings</button> : <button className="button secondary compact" disabled={!!busy || loading || meeting.status !== "recording" || !selected.length} onClick={() => void action("set_live_coach", { enabled: true, agentIds: selected })}>{busy === "set_live_coach" ? "Turning on…" : "Turn on for this call"}</button>}
        <button className="live-coach-manage" onClick={onSettings}>Manage agents</button>
      </>}
      {(error || backgroundUnavailable) && <div className="live-coach-error" role="status"><p>{error || coach.message}</p><p>{ended ? "Your meeting is saved." : meeting.status === "paused" ? "Recording is paused." : "Recording continues."} Manual chat uses the same AI connection.</p>{backgroundUnavailable ? <><p>Live coach checks again automatically while it stays on; reconnect your AI provider and allow a little time.</p><button className="live-coach-manage" onClick={onSettings}>Check AI settings</button></> : !busy && retry.current && <button className="live-coach-manage" onClick={() => { const next = retry.current; if (next) void action(next.method, next.params); }}>Try again</button>}</div>}
      {items.length > 0 && <ol className="live-coach-suggestions">{items.map((item) => <li key={item.id}>
        <div className="live-coach-source"><span>{item.agentName} · {item.kind === "question" ? "Ask next" : item.kind === "follow_up" ? "Follow-up" : item.kind === "insight" ? "Insight" : "Remember"}</span><button className="live-coach-citation" aria-label={`View transcript at ${clock(item.start)}`} onClick={() => onSource(item.start)}>{clock(item.start)}</button><button className="icon-button" aria-label={`Dismiss suggestion: ${item.text}`} disabled={!!busy} onClick={() => void action("dismiss_coach_suggestion", { id: item.id })}><X size={14} /></button></div>
        <p>{item.text}</p>
        {(item.kind === "follow_up" || item.kind === "reminder") && <button className="button subtle compact" disabled={!!busy} onClick={() => void action("save_coach_follow_up", { id: item.id })}>{busy === "save_coach_follow_up" ? "Saving…" : "Save follow-up"}</button>}
      </li>)}</ol>}
      {!items.length && coach.enabled && !ended && coach.status !== "unavailable" && <p className="live-coach-quiet">{meeting.status === "paused" ? "Resume recording to continue." : "Tips appear here quietly. Nothing to act on yet."}</p>}
      {notice && <p className="live-coach-saved" role="status"><Check size={13} />{notice}</p>}
      {!notice && saved > 0 && <p className="live-coach-saved"><Check size={13} />{saved === 1 ? "1 follow-up saved to Tasks" : `${saved} follow-ups saved to Tasks`}</p>}
    </section>
  );
}
