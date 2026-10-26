import { useEffect, useRef, useState } from "react";
import { call, subscribe } from "../lib/bridge";

export function shortcutLabel(value: string, mac: boolean) {
  return value.split("+").map(part => {
    const names: Record<string, string> = { commandorcontrol: mac ? "Cmd" : "Ctrl", super: "Cmd", meta: "Cmd", command: "Cmd", control: "Ctrl", ctrl: "Ctrl", alt: mac ? "Option" : "Alt", shift: "Shift", space: "Space" };
    return names[part.toLowerCase()] || part.replace(/^Key|^Digit/, "");
  }).join(" + ");
}

export default function ShortcutRecorder({ value, disabled, mac, onChange }: {
  value: string; disabled: boolean; mac: boolean; onChange: (value: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [ready, setReady] = useState(false);
  const [message, setMessage] = useState("");
  const button = useRef<HTMLButtonElement>(null);
  const change = useRef(onChange);
  change.current = onChange;
  useEffect(() => { if (disabled) setRecording(false); }, [disabled]);
  useEffect(() => {
    if (!recording || disabled) return;
    let live = true;
    let unlisten: (() => void) | undefined;
    const stop = () => { live = false; setRecording(false); };
    const accept = (shortcut: string) => {
      if (!live) return;
      const parts = shortcut.toLowerCase().split("+");
      const primary = parts.some(p => ["super", "meta", "command", "commandorcontrol", "control", "ctrl", "alt", "option"].includes(p));
      if (!primary) { setMessage("Include Command, Control, or Option/Alt with a key."); return; }
      if (parts.includes("shift") && parts.some(p => [mac ? "super" : "control", "commandorcontrol", mac ? "command" : "ctrl"].includes(p)) && parts.some(p => p === "r" || p === "keyr")) {
        setMessage("That shortcut starts a meeting. Choose another shortcut for dictation."); return;
      }
      change.current(shortcut);
      setMessage("Shortcut captured. Save dictation settings to apply it.");
      stop();
    };
    const keydown = (event: KeyboardEvent) => {
      if (!live) return;
      if (event.key === "Tab" && !event.metaKey && !event.ctrlKey && !event.altKey) { stop(); return; }
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.key === "Escape") { setMessage("Shortcut recording canceled."); stop(); return; }
      if (event.repeat || event.isComposing || ["Meta", "Control", "Alt", "Shift"].includes(event.key)) return;
      const code = event.code;
      if (!/^(Key[A-Z]|Digit[0-9]|F([1-9]|1[0-9]|2[0-4])|Space|Enter|Tab|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Arrow(Up|Down|Left|Right)|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Backquote)$/.test(code)) {
        setMessage("Choose a letter, number, function key, or standard keyboard key."); return;
      }
      const key = code.replace(/^Key|^Digit/, "");
      accept([event.metaKey && "Super", event.ctrlKey && "Control", event.altKey && "Alt", event.shiftKey && "Shift", key].filter(Boolean).join("+"));
    };
    const blur = () => { setMessage("Shortcut recording canceled."); stop(); };
    void (async () => {
      try {
        unlisten = await subscribe(event => {
          if (event.event === "shortcut_recorded" && typeof event.payload.shortcut === "string") accept(event.payload.shortcut);
        });
        if (!live) { unlisten(); return; }
        await call("shortcut_recorder", { active: true });
        if (!live) return;
        window.addEventListener("keydown", keydown, true);
        window.addEventListener("blur", blur);
        setReady(true);
      } catch {
        if (live) { setMessage("Shortcut recording could not start. Try again."); stop(); }
      }
    })();
    return () => {
      live = false;
      unlisten?.();
      window.removeEventListener("keydown", keydown, true);
      window.removeEventListener("blur", blur);
      void call("shortcut_recorder", { active: false }).catch(() => {});
    };
  }, [recording, disabled, mac]);
  return <div className="dictation-shortcut-recorder">
    <span id="dictation-shortcut-label">Global shortcut</span>
    <div className="dictation-shortcut-fields">
      <output aria-labelledby="dictation-shortcut-label">{shortcutLabel(value, mac)}</output>
      <button ref={button} type="button" className="button secondary compact" disabled={disabled}
        aria-pressed={recording} aria-describedby="dictation-shortcut-help"
        onBlur={() => setRecording(false)}
        onClick={event => { event.currentTarget.focus(); setReady(false); setMessage(""); setRecording(current => !current); }}>
        {recording ? "Cancel recording" : "Record shortcut"}
      </button>
    </div>
    <p id="dictation-shortcut-help" className="quiet-note" role="status">{recording ? (message || (ready ? "Press your shortcut now. Escape cancels." : "Preparing shortcut recorder…")) : (message || "Record a key with Command, Control, or Option/Alt, then save your settings.")}</p>
  </div>;
}
