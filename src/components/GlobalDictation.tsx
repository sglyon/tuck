import { useEffect, useState } from "react";
import { ArrowUpRight, LoaderCircle, Mic, Square, X } from "lucide-react";
import { call, native } from "../lib/bridge";
import ShortcutRecorder, { shortcutLabel } from "./ShortcutRecorder";
import type { GlobalDictationConfig } from "../lib/types";
import type { GlobalDictationController } from "../lib/useGlobalDictation";

const DEFAULT_CLEANUP_PROMPT = "Clean up punctuation, capitalization, spacing, and paragraph breaks. Remove filler sounds such as um and uhhh. Preserve every other word in its original order. Do not paraphrase, correct grammar by changing words, add headings with new words, or add commentary. Return only the formatted transcript.";

function statusLabel(state: string) {
  switch (state) {
    case "starting": return "Getting your microphone ready…";
    case "listening": return "Listening · microphone only";
    case "processing": return "Finishing your voice note…";
    case "cleaning": return "Formatting your words…";
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
  const { status, pending, error, refresh, toggle, cancel, configure } = controller;
  const [draft, setDraft] = useState<GlobalDictationConfig>({ enabled: true, shortcut: "CommandOrControl+Shift+Space", autoPaste: true, cleanupEnabled: false, cleanupPrompt: DEFAULT_CLEANUP_PROMPT });
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (status) setDraft({ enabled: status.enabled !== false, shortcut: status.shortcut, autoPaste: status.autoPaste !== false, cleanupEnabled: status.cleanupEnabled === true, cleanupPrompt: status.cleanupPrompt || DEFAULT_CLEANUP_PROMPT });
  }, [status?.enabled, status?.shortcut, status?.autoPaste, status?.cleanupEnabled, status?.cleanupPrompt]);
  const [permissionBusy, setPermissionBusy] = useState(false);
  const [permissionError, setPermissionError] = useState("");
  useEffect(() => { void refresh(); }, [refresh]);
  const active = status && ["starting", "listening", "processing", "cleaning"].includes(status.state);
  const mac = /mac|darwin/i.test(platform);
  const needsAccessibility = mac && ["denied", "unknown"].includes(status?.pastePermission || "unknown");
  const locked = !!pending || !!active;
  const enabled = status?.enabled !== false;
  const edit = (changes: Partial<GlobalDictationConfig>) => { setDraft(d => ({...d,...changes})); setSaved(false); };
  return <section className="global-dictation-settings" aria-label="Dictation anywhere">
    <div className="section-heading"><Mic size={18} /><h2>Dictation anywhere</h2></div>
    <p className="section-description">Keep your cursor where you’re working. Speak into Tuck, then bring your words back to that app.</p>
    {status && <>
      <fieldset className="dictation-preferences" disabled={locked || !native}>
        <legend>Dictation preferences</legend>
        <label className="dictation-preference-toggle"><input type="checkbox" role="switch" checked={draft.enabled} onChange={e => edit({enabled:e.target.checked})} /><span>Enable dictation anywhere<small>Use a global shortcut or the tray menu. Voice notes inside Tuck stay available when this is off.</small></span></label>
        <ShortcutRecorder value={draft.shortcut} disabled={locked || !native} mac={mac} onChange={shortcut => edit({shortcut})} />
        <label className="dictation-preference-toggle"><input type="checkbox" role="switch" checked={draft.autoPaste} onChange={e => edit({autoPaste:e.target.checked})} /><span>Paste automatically<small>{mac ? "Send a paste only to the original confirmed text field. Otherwise, copy for manual paste." : "Automatic paste is currently supported on macOS. This device copies for manual paste."}</small></span></label>
        {!draft.autoPaste && <p className="quiet-note">Clipboard only: Tuck will copy your words without inspecting a text field or sending a paste.</p>}
        <label className="dictation-preference-toggle"><input type="checkbox" role="switch" checked={draft.cleanupEnabled} onChange={event => edit({cleanupEnabled:event.target.checked})} /><span>Format dictated text with AI<small>Optional punctuation and formatting using the provider and model in Intelligence settings. Hosted AI also requires your hosted-processing consent.</small></span></label>
        {draft.cleanupEnabled && <div className="dictation-cleanup-preferences">
          <label className="dictation-cleanup-prompt">Cleanup prompt<textarea rows={5} maxLength={2000} value={draft.cleanupPrompt} onChange={event => edit({cleanupPrompt:event.target.value})} aria-describedby="dictation-cleanup-rules" /></label>
          <p id="dictation-cleanup-rules" className="quiet-note">Every word must stay in order. Only punctuation, capitalization, spacing, and removal of “um”/“uh” filler sounds are allowed. Tuck checks the result and keeps the original if words change. Long dictations and unavailable or slow AI also keep the original.</p>
          <button className="global-dictation-link" onClick={() => edit({cleanupPrompt:status.cleanupDefaultPrompt || DEFAULT_CLEANUP_PROMPT})}>Reset cleanup prompt</button>
        </div>}
        <div className="global-dictation-actions">
          <button className="button secondary compact" onClick={async () => { if (await configure(draft)) setSaved(true); }}>Save dictation settings</button>
          <button className="global-dictation-link" onClick={() => edit({enabled:true,shortcut:"CommandOrControl+Shift+Space",autoPaste:true,cleanupEnabled:false,cleanupPrompt:status.cleanupDefaultPrompt || DEFAULT_CLEANUP_PROMPT})}>Restore defaults</button>
        </div>
      </fieldset>
      {active && <p className="quiet-note">Finish dictation before changing these settings.</p>}
      {saved && <p className="quiet-note" role="status">Dictation settings saved on this device.</p>}
      {!enabled && <p className="quiet-note">Dictation anywhere is off. Its global shortcut is released.</p>}
      <div className="global-dictation-shortcut"><kbd>{shortcutLabel(status.shortcut, mac)}</kbd><span>{enabled ? "Press once to speak. Press again to finish." : "Shortcut inactive while dictation is off."}</span></div>
      <p className="global-dictation-instructions">Click a text field in another app, then use the shortcut. Tuck stays in the background and saves a local voice note.</p>
      <p className="global-dictation-privacy">{status.cleanupEnabled ? "Microphone only. Local speech recognition. Optional formatting uses your selected AI provider." : "Microphone only. Local transcription. No AI rewriting."}</p>
      {enabled && !status.shortcutAvailable && <p className="global-dictation-warning">{status.message || "The shortcut could not be registered. Another app may be using it. You can still start dictation here and paste the copied text yourself."}</p>}
      {status.pasteAvailable ? <p className="quiet-note">Your words are copied, then a paste is sent only when Tuck can confirm your original text field.</p> : <p className="quiet-note">Your words are copied to the clipboard. Paste them yourself when automatic insertion is unavailable.</p>}
      {enabled && status.autoPaste !== false && needsAccessibility && !status.pasteAvailable && <div className="global-dictation-permission">
        <p>To send a paste to your text field, allow Tuck in macOS Accessibility. Recording and copying work without this permission.</p>
        <button className="button secondary compact" disabled={permissionBusy || !!active} onClick={async () => {
          setPermissionBusy(true); setPermissionError("");
          try { await call("dictation_accessibility_settings"); }
          catch (e) { setPermissionError(e instanceof Error ? e.message : String(e)); }
          finally { setPermissionBusy(false); }
        }}>Open Accessibility settings <ArrowUpRight size={13} /></button>
        <button className="global-dictation-link" disabled={!!pending} onClick={() => void refresh()}>Check again</button>
      </div>}
      {status.cleanupMessage && <p className="quiet-note" role="status">{status.cleanupMessage}</p>}
      <div className="global-dictation-result"><p>{statusLabel(status.state)}</p>{status.shortcutAvailable && status.message && <p>{status.message}</p>}</div>
      <div className="global-dictation-actions">
        <button className="button secondary compact" disabled={!enabled || !!pending || status.state === "starting" || (status.state === "processing" || status.state === "cleaning")} onClick={() => void toggle()}>{status.state === "listening" ? <><Square size={13} />Finish dictation</> : <><Mic size={14} />Start dictation here</>}</button>
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
  if (!status || dismissed || !["starting", "listening", "processing", "cleaning", "error"].includes(status.state)) return null;
  const active = status.state !== "error";
  return <aside className={`global-dictation-indicator${active ? "" : " has-error"}`} aria-label="Global dictation status">
    {["processing", "cleaning"].includes(status.state) ? <LoaderCircle size={16} className="spin" /> : <Mic size={16} />}
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
