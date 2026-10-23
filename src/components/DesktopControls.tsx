import { useState } from "react";
import { Check, LoaderCircle, Mic } from "lucide-react";
import { call, native } from "../lib/bridge";
import type { Settings } from "../lib/types";
type Device = { id: string; name: string; isDefault: boolean };
type Readiness = {
  modelReady: boolean;
  helperReady: boolean;
  microphoneAvailable: boolean;
  systemAvailable: boolean;
  issues: { code: string; message: string }[];
};
export default function DesktopControls({
  settings,
  onUpdate,
  notify,
}: {
  settings: Settings;
  onUpdate: (v: Partial<Settings>) => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [devices, setDevices] = useState<{
    microphones: Device[];
    systemOutputs: Device[];
  } | null>(null);
  const [ready, setReady] = useState<Readiness | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="desktop-controls">
      <div className="section-heading">
        <Mic size={18} />
        <h2>Before your next call</h2>
      </div>
      <p className="section-description">
        Check your devices, then run a five-second sound check. Test audio is
        never saved or transcribed.
      </p>
      <button
        className="button secondary compact"
        disabled={busy || !native}
        onClick={() =>
          void run(async () => {
            setReady(await call<Readiness>("capture_readiness"));
            setDevices(await call("capture_devices"));
          })
        }
      >
        {busy ? (
          <LoaderCircle className="spin" size={14} />
        ) : (
          <Check size={14} />
        )}
        Check recording setup
      </button>
      {!native && (
        <p className="quiet-note">
          Device checks are available in the desktop app.
        </p>
      )}
      {ready && (
        <div className="readiness-result" role="status">
          <p>
            {ready.modelReady &&
            ready.helperReady &&
            ready.microphoneAvailable &&
            ready.systemAvailable
              ? "Your recording setup is ready."
              : "A few things need attention before recording."}
          </p>
          {ready.issues.map((i) => (
            <p key={i.code}>{i.message}</p>
          ))}
        </div>
      )}
      {devices && (
        <div className="device-fields">
          {(
            [
              ["microphoneDeviceId", "Microphone", devices.microphones],
              ["systemDeviceId", "Computer audio", devices.systemOutputs],
            ] as const
          ).map(([field, label, list]) => (
            <label key={field}>
              {label}
              <select
                aria-label={label + " device"}
                value={settings[field]}
                onChange={(e) =>
                  void run(() => onUpdate({ [field]: e.target.value }))
                }
              >
                <option value="">System default</option>
                {list.map((d) => (
                  <option value={d.id} key={d.id}>
                    {d.name}
                    {d.isDefault ? " (default)" : ""}
                  </option>
                ))}
              </select>
            </label>
          ))}
          <button
            className="button subtle compact"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const r = await call<{
                  microphone: { heard: boolean };
                  system: { heard: boolean };
                  warnings: string[];
                }>("test_audio", { microphone: true, systemAudio: true });
                setResult(
                  `Microphone: ${r.microphone.heard ? "sound detected" : "no sound detected"}. Computer audio: ${r.system.heard ? "sound detected" : "no sound detected"}. ${r.warnings.join(" ")}`,
                );
              })
            }
          >
            {busy ? "Checking sound…" : "Test both audio sources"}
          </button>
          <p className="quiet-note">
            Speak and play a sound on your computer during the test.
          </p>
          {result && <p role="status">{result}</p>}
        </div>
      )}
    </section>
  );
}
