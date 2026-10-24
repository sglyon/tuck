import { useEffect, useState } from "react";
import { ArrowUpRight, LoaderCircle, Mic, Square, X } from "lucide-react";
import { call, native } from "../lib/bridge";
import type { GlobalDictationController } from "../lib/useGlobalDictation";

function shortcutLabel(shortcut: string) {
  return shortcut.replace("CommandOrControl", /Mac/.test(navigator.platform) ? "Cmd" : "Ctrl").replace(/\+/g, " + ");
}
function statusLabel(state: string) {
  switch (state) {
    case "starting": return "Getting your microphone ready…";
    case "listening": return "Listening · microphone only";
    case "processing": return "Finishing your voice note…";
    case "copied": return "Copied. Paste your words wherever you need them.";
    case "paste_sent": return "Copied. Paste sent to your text field.";
    case "error": return "Dictation needs attention";
    default: return "Not recording";
  }
}

export default function GlobalDictation({ controller, platform, onOpenNote }: {
  controller: GlobalDictationController;
  platform: string;
  onOpenNote: (id: string) => void;
}) {
  const { status, pending, error, refresh, toggle, cancel } = controller;
  const [permissionBusy, setPermissionBusy] = useState(false);
  const [permissionError, setPermissionError] = useState("");
  useEffect(() => { void refresh(); }, [refresh]);
  const active = status && ["starting", "listening", "processing"].includes(status.state);
  const mac = /mac|darwin/i.test(platform);
  const needsAccessibility = mac && ["denied", "unknown"].includes(status?.pastePermission || "unknown");
  return <section className="global-dictation-settings" aria-label="Dictation anywhere">
    <div className="section-heading"><Mic size={18} /><h2>Dictation anywhere</h2></div>
    <p className="section-description">Keep your cursor where you’re working. Speak into Tuck, then bring your words back to that app.</p>
    {status && <>
      <div className="global-dictation-shortcut"><kbd>{shortcutLabel(status.shortcut)}</kbd><span>Press once to speak. Press again to finish.</span></div>
      <p className="global-dictation-instructions">Click a text field in another app, then use the shortcut. Tuck stays in the background and saves a local voice note.</p>
      <p className="global-dictation-privacy">Microphone only. Local transcription. No AI rewriting.</p>
      {!status.shortcutAvailable && <p className="global-dictation-warning">{status.message || "The shortcut could not be registered. Another app may be using it. You can still start dictation here and paste the copied text yourself."}</p>}
      {status.pasteAvailable ? <p className="quiet-note">Your words are copied, then a paste is sent only when Tuck can confirm your original text field.</p> : <p className="quiet-note">Your words are copied to the clipboard. Paste them yourself when automatic insertion is unavailable.</p>}
      {needsAccessibility && !status.pasteAvailable && <div className="global-dictation-permission">
        <p>To send a paste to your text field, allow Tuck in macOS Accessibility. Recording and copying work without this permission.</p>
        <button className="button secondary compact" disabled={permissionBusy || !!active} onClick={async () => {
          setPermissionBusy(true); setPermissionError("");
          try { await call("dictation_accessibility_settings"); }
          catch (e) { setPermissionError(e instanceof Error ? e.message : String(e)); }
          finally { setPermissionBusy(false); }
        }}>Open Accessibility settings <ArrowUpRight size={13} /></button>
        <button className="global-dictation-link" disabled={!!pending} onClick={() => void refresh()}>Check again</button>
      </div>}
      <div className="global-dictation-result"><p>{statusLabel(status.state)}</p>{status.shortcutAvailable && status.message && <p>{status.message}</p>}</div>
      <div className="global-dictation-actions">
        <button className="button secondary compact" disabled={!!pending || status.state === "starting" || status.state === "processing"} onClick={() => void toggle()}>{status.state === "listening" ? <><Square size={13} />Finish dictation</> : <><Mic size={14} />Start dictation here</>}</button>
        {active && <button className="button subtle compact" disabled={!!pending} onClick={() => void cancel()}>Save without inserting</button>}
        {!active && status.meetingId && <button className="global-dictation-link" onClick={() => onOpenNote(status.meetingId!)}>Open voice note</button>}
      </div>
      {!active && <p className="quiet-note">Starting here saves and copies a voice note. Use the shortcut from another app to send a paste there.</p>}
    </>}
    {(!status || error || permissionError) && <p className="global-dictation-warning" role={permissionError ? "status" : undefined}>{error || permissionError || (native ? "Checking dictation…" : "Global shortcuts and microphone capture are available in the desktop app.")}</p>}
    {error && <button className="button secondary compact" disabled={!!pending} onClick={() => void refresh()}>Retry dictation check</button>}
  </section>;
}

/** Always mounted outside the pane switches so a completion remains announced. */
export function GlobalDictationAnnouncer({ controller }: { controller: GlobalDictationController }) {
  const { status, error } = controller;
  const label = status && status.state !== "idle" ? statusLabel(status.state) : "";
  const detail = error || status?.message || "";
  const text = [label, detail === label ? "" : detail].filter(Boolean).join(" ");
  return <p className="visually-hidden" role="status" aria-label="Dictation announcements" aria-live="polite" aria-atomic="true">{text}</p>;
}

export function GlobalDictationIndicator({ controller, onSetup, onOpenNote }: {
  controller: GlobalDictationController;
  onSetup: () => void;
  onOpenNote: (id: string) => void;
}) {
  const { status, pending, error, dismissed, toggle, cancel, dismiss } = controller;
  if (!status || dismissed || !["starting", "listening", "processing", "error"].includes(status.state)) return null;
  const active = status.state !== "error";
  return <aside className={`global-dictation-indicator${active ? "" : " has-error"}`} aria-label="Global dictation status">
    {status.state === "processing" ? <LoaderCircle size={16} className="spin" /> : <Mic size={16} />}
    <div><strong>{statusLabel(status.state)}</strong>{(error || status.message) && <p>{error || status.message}</p>}</div>
    <div className="global-dictation-indicator-actions">
      {status.state === "listening" && <button className="button secondary compact" disabled={!!pending} onClick={() => void toggle()}>Finish dictation</button>}
      {active ? <button className="global-dictation-link" disabled={!!pending} onClick={() => void cancel()}>Save without inserting</button> : <>
        <button className="global-dictation-link" onClick={onSetup}>Check setup</button>
        {status.meetingId && <button className="global-dictation-link" onClick={() => onOpenNote(status.meetingId!)}>Open voice note</button>}
        <button className="icon-button" aria-label="Dismiss dictation status" onClick={dismiss}><X size={15} /></button>
      </>}
    </div>
  </aside>;
}
