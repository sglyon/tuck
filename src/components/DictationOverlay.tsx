import { useEffect, useState } from "react";
import { Check, CircleAlert, LoaderCircle, Mic } from "lucide-react";
import { call, subscribe } from "../lib/bridge";
import type { GlobalDictationStatus } from "../lib/types";
import { shortcutLabel } from "./ShortcutRecorder";
import "./dictation-overlay.css";

export default function DictationOverlay() {
  const [status, setStatus] = useState<GlobalDictationStatus | null>(null);
  useEffect(() => {
    let live = true;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    document.documentElement.classList.add("dictation-overlay-window");
    void (async () => {
      try {
        unlisten = await subscribe(event => {
          if (live && event.event === "global_dictation_updated") {
            revision++;
            setStatus(event.payload as unknown as GlobalDictationStatus);
          }
        });
        if (!live) { unlisten(); return; }
        const stamp = revision;
        const initial = await call<GlobalDictationStatus>("global_dictation_status");
        if (live && revision === stamp) setStatus(initial);
      } catch { /* Native window stays hidden until a dictation event arrives. */ }
    })();
    return () => { live = false; unlisten?.(); document.documentElement.classList.remove("dictation-overlay-window"); };
  }, []);
  if (!status || status.state === "idle") return null;
  const finishing = status.state === "starting" || status.state === "processing" || status.state === "cleaning";
  const failed = status.state === "error";
  const listening = status.state === "listening";
  const title = { starting: "Getting ready…", listening: "Listening", processing: "Transcribing…", cleaning: "Formatting…", copied: "Copied to clipboard", paste_sent: "Paste sent", error: "Dictation needs attention" }[status.state];
  const detail = listening ? `${shortcutLabel(status.shortcut, /Mac/.test(navigator.platform))} to finish`
    : status.state === "copied" ? "Your words are ready to paste."
    : status.state === "paste_sent" ? "Your words are also on the clipboard."
    : status.state === "cleaning" ? "Preserving your words."
    : failed ? "Open Tuck for details."
    : status.state === "starting" ? "Preparing your microphone." : "Finishing on this device.";
  const Icon = finishing ? LoaderCircle : failed ? CircleAlert : listening ? Mic : Check;
  return <div className="dictation-hud" role="status" aria-live="polite" data-state={status.state}>
    <div className="dictation-hud-icon"><Icon size={23} className={finishing ? "dictation-hud-spin" : undefined} /></div>
    <div className="dictation-hud-copy"><strong>{title}</strong><span>{detail}</span></div>
  </div>;
}
