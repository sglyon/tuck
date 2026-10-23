import { useState, useEffect, type ReactNode, type KeyboardEvent } from "react";
import {
  ArrowUpRight,
  AudioLines,
  Check,
  ChevronRight,
  Cloud,
  Cpu,
  Database,
  Download,
  ExternalLink,
  HardDrive,
  LoaderCircle,
  LogOut,
  Mic,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Unplug,
} from "lucide-react";
import BackupRestore from "./BackupRestore";
import NotionConnection from "./NotionConnection";
import SavedWriting from "./SavedWriting";
import MeetingAgents from "./MeetingAgents";
import DesktopControls from "./DesktopControls";
import { call, native, openExternal } from "../lib/bridge";
import type {
  Settings as SettingsData,
  Account,
  EngineStatus,
  ModelOption,
} from "../lib/types";
type Props = {
  initialTab?: string;
  meetingCount: number;
  beforeFileOperation: () => Promise<boolean>;
  onLibraryChanged: () => Promise<void>;
  settings: SettingsData;
  accounts: Account[];
  status: EngineStatus;
  onUpdate: (changes: Partial<SettingsData>) => Promise<void>;
  onAccounts: (accounts: Account[]) => void;
  onStatus: () => Promise<void>;
  notify: (message: string, error?: boolean) => void;
};
function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="switch"
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}
function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div>
        <h3>{title}</h3>
        {description && <p>{description}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}
export default function Settings({
  initialTab = "Intelligence",
  meetingCount,
  beforeFileOperation,
  onLibraryChanged,
  settings: s,
  accounts,
  status,
  onUpdate,
  onAccounts,
  onStatus,
  notify,
}: Props) {
  const [tab, setTab] = useState(initialTab);
  useEffect(() => setTab(initialTab), [initialTab]);
  const [busy, setBusy] = useState<string | null>(null);
  const [openaiKey, setOpenaiKey] = useState("");
  const [notionToken, setNotionToken] = useState("");
  const [notionSelection, setNotionSelection] = useState(s.notionContent || {recap:true, notes:true, tasks:true});
  useEffect(() => setNotionSelection(s.notionContent || {recap:true, notes:true, tasks:true}), [s.notionContent]);
  const [localUrl, setLocalUrl] = useState(s.localUrl);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [customModel, setCustomModel] = useState(s.model);
  useEffect(() => setLocalUrl(s.localUrl), [s.localUrl]);
  useEffect(() => setCustomModel(s.model), [s.model]);
  useEffect(() => setModels([]), [s.provider]);
  const run = async (name: string, fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(name);
    try {
      await fn();
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
    } finally {
      setBusy(null);
    }
  };
  const change = (v: Partial<SettingsData>) =>
    void run("settings", () => onUpdate(v));
  const saveSecret = (kind: "openai" | "notion", value: string) =>
    run(kind, async () => {
      if (!value.trim()) return;
      await call("save_secret", { kind, value: value.trim() });
      kind === "openai" ? setOpenaiKey("") : setNotionToken("");
      await onUpdate({});
      notify(
        `${kind === "openai" ? "OpenAI key" : "Notion token"} saved in your system credential store.`,
      );
    });
  const install = (kind: "parakeet" | "speakers") =>
    run(kind, async () => {
      await call("install_model", { kind });
      await onStatus();
      notify(
        kind === "parakeet"
          ? "Parakeet is ready. You can record your first conversation."
          : "Speaker separation is ready. Names remain yours to assign.",
      );
    });
  const pending = (name: string) => busy === name;
  return (
    <section className="settings-page">
      <header className="settings-heading">
        <div className="page-icon">
          <ShieldCheck size={23} />
        </div>
        <h1>Make Tuck yours.</h1>
        <p>Local by default. Connected when you choose.</p>
      </header>
      <div
        className="settings-tabs"
        onKeyDown={tabNavigation}
        role="tablist"
        aria-label="Settings sections"
      >
        {["Intelligence", "Audio & models", "Writing", "Notion", "Privacy"].map(
          (t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ),
        )}
      </div>
      <div className="settings-content" role="tabpanel" aria-label={tab}>
        {tab === "Writing" && (
          <SavedWriting
            settings={s}
            onRefresh={() => onUpdate({})}
            notify={notify}
          />
        )}
        {tab === "Intelligence" && (
          <>
            <div className="section-heading">
              <Sparkles size={18} />
              <h2>Your thinking partner</h2>
            </div>
            <p className="section-description">
              Choose what powers recaps and the assistant. Speech recognition
              always runs on your device.
            </p>
            <div
              className="provider-options"
              role="group"
              aria-label="AI provider"
            >
              {[
                {
                  id: "local",
                  name: "On this device",
                  caption: "Private, with Ollama",
                  Icon: HardDrive,
                },
                {
                  id: "chatgpt",
                  name: "ChatGPT",
                  caption: "Use your eligible plan",
                  Icon: Sparkles,
                },
                {
                  id: "openai",
                  name: "OpenAI API",
                  caption: "Bring your own key",
                  Icon: Cloud,
                },
              ].map(({ id, name, caption, Icon }) => (
                <button
                  className={s.provider === id ? "selected" : ""}
                  key={id}
                  disabled={!!busy}
                  onClick={() =>
                    change({
                      provider: id as SettingsData["provider"],
                      model: id === "local" ? "qwen3:4b" : "",
                    })
                  }
                >
                  <Icon size={20} />
                  <span>
                    <strong>{name}</strong>
                    <small>{caption}</small>
                  </span>
                  {s.provider === id && <Check size={16} />}
                </button>
              ))}
            </div>
            {s.provider === "local" && (
              <div className="provider-detail">
                <h3>Keep your conversations on this computer.</h3>
                <p>
                  Run an Ollama model locally. Smaller models use less memory;
                  transcription does not need an LLM running.
                </p>
                <details className="advanced-settings">
                  <summary>Advanced connection settings</summary>
                  <label className="field-label" htmlFor="ollama-url">
                    Ollama address
                  </label>
                  <div className="inline-input">
                    <input
                      id="ollama-url"
                      type="url"
                      value={localUrl}
                      onChange={(e) => setLocalUrl(e.target.value)}
                      spellCheck={false}
                    />
                    <button
                      className="button secondary"
                      disabled={!!busy || localUrl === s.localUrl}
                      onClick={() => change({ localUrl })}
                    >
                      Save
                    </button>
                  </div>
                </details>
                <a
                  className="text-link"
                  href="https://ollama.com/download"
                  onClick={(e) => {
                    e.preventDefault();
                    void openExternal("https://ollama.com/download");
                  }}
                >
                  Get Ollama <ArrowUpRight size={13} />
                </a>
              </div>
            )}
            {s.provider === "chatgpt" && (
              <div className="provider-detail">
                <h3>Bring your ChatGPT plan.</h3>
                <p>
                  Sign in in your browser and choose whether to allow plan
                  usage. Tuck cannot read your ChatGPT conversations.
                  Eligibility and usage limits apply.
                </p>
                <div className="account-list">
                  {accounts.map((a) => (
                    <div className="account-row" key={a.id}>
                      <div>
                        <strong>{a.email || a.label}</strong>
                        <small>
                          {a.connected
                            ? a.sharing
                              ? "Plan usage enabled"
                              : "Identity connected · plan usage unavailable"
                            : "Signed out"}
                          {a.active ? " · Active" : ""}
                        </small>
                      </div>
                      <div className="account-actions">
                        {!a.active && a.connected && (
                          <button
                            className="button subtle compact"
                            onClick={() =>
                              void run("account", async () =>
                                onAccounts(
                                  await call<Account[]>("chatgpt_select", {
                                    id: a.id,
                                  }),
                                ),
                              )
                            }
                          >
                            Use
                          </button>
                        )}
                        {a.connected && (
                          <button
                            className="icon-button"
                            title="Sign out of this account"
                            aria-label={`Sign out ${a.email}`}
                            onClick={() =>
                              void run("account", async () => {
                                const r = await call<{
                                  accounts: Account[];
                                  remoteRevoked: boolean;
                                }>("chatgpt_sign_out", { id: a.id });
                                onAccounts(r.accounts);
                                notify(
                                  r.remoteRevoked
                                    ? "Signed out."
                                    : "Signed out locally. Remote revocation wasn't confirmed; review app access in ChatGPT Settings.",
                                );
                              })
                            }
                          >
                            <LogOut size={16} />
                          </button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
                <button
                  className="button chatgpt-button"
                  disabled={!!busy}
                  onClick={() =>
                    void run("signin", async () => {
                      onAccounts(await call<Account[]>("chatgpt_sign_in"));
                      notify(
                        "ChatGPT connected. Load available models to choose one.",
                      );
                    })
                  }
                >
                  {pending("signin") ? (
                    <LoaderCircle className="spin" size={18} />
                  ) : (
                    <Sparkles size={18} />
                  )}
                  Continue with ChatGPT
                </button>
                {pending("signin") && (
                  <p className="hint" role="status">
                    Finish connecting in your browser. You can keep this window
                    open.
                  </p>
                )}
                <a
                  className="text-link"
                  href="https://chatgpt.com/settings/usage"
                  onClick={(e) => {
                    e.preventDefault();
                    void openExternal("https://chatgpt.com/settings/usage");
                  }}
                >
                  Manage usage <ExternalLink size={13} />
                </a>
              </div>
            )}
            {s.provider === "openai" && (
              <div className="provider-detail">
                <h3>Your key, your account.</h3>
                <p>
                  Usage is billed to your OpenAI API account. Keys stay in the
                  system credential store and never enter browser storage.
                </p>
                <label className="field-label" htmlFor="openai-key">
                  API key{" "}
                  {s.hasOpenAIKey && (
                    <span className="connected-text">
                      <Check size={12} />
                      Saved
                    </span>
                  )}
                </label>
                <form
                  className="inline-input"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void saveSecret("openai", openaiKey);
                  }}
                >
                  <input
                    id="openai-key"
                    type="password"
                    value={openaiKey}
                    onChange={(e) => setOpenaiKey(e.target.value)}
                    placeholder={
                      s.hasOpenAIKey ? "Enter a replacement key" : "sk-…"
                    }
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <button
                    className="button secondary"
                    disabled={!!busy || !openaiKey.trim()}
                  >
                    {pending("openai") ? (
                      <LoaderCircle size={15} className="spin" />
                    ) : (
                      "Save key"
                    )}
                  </button>
                </form>
              </div>
            )}
            {s.provider !== "local" && (
              <SettingRow
                title="Allow hosted transcript processing"
                description="Your transcript and notes go to OpenAI when you ask or generate a recap. Coaching sends recent transcript excerpts, selected agent instructions and earlier tips while enabled. Audio stays local."
              >
                <Toggle
                  label="Allow hosted transcript processing"
                  checked={s.hostedConsent}
                  disabled={!!busy}
                  onChange={(v) =>
                    change({
                      hostedConsent: v,
                      ...(!v ? { webSearch: false } : {}),
                    })
                  }
                />
              </SettingRow>
            )}
            <div className="model-picker">
              <div className="setting-row">
                <div>
                  <h3>AI model</h3>
                  <p>Load models from your selected provider.</p>
                </div>
                <button
                  className="button secondary compact"
                  disabled={!!busy}
                  onClick={() =>
                    void run("models", async () => {
                      const result = await call<ModelOption[]>("list_models");
                      setModels(result);
                      if (!result.length)
                        notify(
                          "No models found. Install a model in Ollama or check your account access.",
                          true,
                        );
                    })
                  }
                >
                  {pending("models") ? (
                    <LoaderCircle size={14} className="spin" />
                  ) : (
                    <RefreshCw size={14} />
                  )}
                  Load models
                </button>
              </div>
              {models.length > 0 ? (
                <select
                  aria-label="AI model"
                  value={s.model}
                  disabled={!!busy}
                  onChange={(e) => change({ model: e.target.value })}
                >
                  <option value="" disabled>
                    Choose a model
                  </option>
                  {models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
              ) : (
                <div className="model-choice-summary">
                  <strong>
                    {s.model === "qwen3:4b"
                      ? "Qwen 3 (4B)"
                      : s.model
                        ? "Saved model"
                        : "No model selected"}
                  </strong>
                  <span>Load models to check what is available.</span>
                </div>
              )}
              <details className="advanced-settings">
                <summary>Advanced model settings</summary>
                <label className="field-label" htmlFor="manual-model">
                  Model ID
                </label>
                <div className="inline-input">
                  <input
                    id="manual-model"
                    aria-label="Model name"
                    value={customModel}
                    onChange={(e) => setCustomModel(e.target.value)}
                    placeholder={
                      s.provider === "local"
                        ? "qwen3:4b"
                        : "Load available models or enter a model ID"
                    }
                    spellCheck={false}
                  />
                  <button
                    className="button secondary"
                    disabled={
                      !!busy || !customModel.trim() || customModel === s.model
                    }
                    onClick={() => change({ model: customModel.trim() })}
                  >
                    Save
                  </button>
                </div>
              </details>
            </div>
            <MeetingAgents agents={s.meetingAgents || []} onUpdate={onUpdate} />
            <SettingRow
              title="Create a recap when a meeting ends"
              description="Automatically create a recap with the provider selected above."
            >
              <Toggle
                label="Create a recap when a meeting ends"
                checked={s.autoSummary}
                disabled={
                  !!busy || (s.provider !== "local" && !s.hostedConsent)
                }
                onChange={(v) => change({ autoSummary: v })}
              />
            </SettingRow>
            <SettingRow
              title="Let the assistant search the web"
              description="For hosted AI only. Search queries may include details from your question and meeting."
            >
              <Toggle
                label="Let the assistant search the web"
                checked={s.webSearch}
                disabled={!!busy || s.provider === "local" || !s.hostedConsent}
                onChange={(v) => change({ webSearch: v })}
              />
            </SettingRow>
          </>
        )}
        {tab === "Audio & models" && (
          <>
            <DesktopControls settings={s} onUpdate={onUpdate} notify={notify} />
            <div className="section-heading">
              <AudioLines size={18} />
              <h2>A small footprint. A clear transcript.</h2>
            </div>
            <p className="section-description">
              Models download only when you ask. No GPU or cloud transcription
              is required.
            </p>
            <div className="model-row">
              <div className="model-glyph">
                <AudioLines size={23} />
              </div>
              <div>
                <h3>Private transcription</h3>
                <p>Speech stays on this computer.</p>
                <small>About 650 MB · downloaded once · works offline</small>
              </div>
              <button
                className={`button ${status.modelInstalled ? "secondary" : "primary"} compact`}
                disabled={
                  !!busy || status.modelDownloading || status.modelInstalled
                }
                onClick={() => void install("parakeet")}
              >
                {status.modelInstalled ? (
                  <>
                    <Check size={15} />
                    Installed
                  </>
                ) : pending("parakeet") || status.modelDownloading ? (
                  <>
                    <LoaderCircle size={15} className="spin" />
                    Installing…
                  </>
                ) : (
                  <>
                    <Download size={15} />
                    Set up
                  </>
                )}
              </button>
            </div>
            {status.modelDownloading && (
              <div className="download-progress" role="status">
                <progress
                  max={1}
                  value={
                    status.modelProgress > 1
                      ? status.modelProgress / 100
                      : status.modelProgress
                  }
                />
                <span>
                  {Math.round(
                    status.modelProgress > 1
                      ? status.modelProgress
                      : status.modelProgress * 100,
                  )}
                  % · You can keep using your library.
                </span>
              </div>
            )}
            <div className="model-row">
              <div className="model-glyph muted-glyph">
                <Mic size={22} />
              </div>
              <div>
                <h3>
                  Speaker separation <span className="small-tag">OPTIONAL</span>
                </h3>
                <p>Group remote voices into provisional speakers.</p>
                <small>
                  You assign names. Overlapping voices may need correction.
                </small>
              </div>
              <button
                className="button secondary compact"
                disabled={!!busy || status.speakerModelInstalled}
                onClick={() => void install("speakers")}
              >
                {status.speakerModelInstalled ? (
                  <>
                    <Check size={15} />
                    Installed
                  </>
                ) : pending("speakers") ? (
                  <LoaderCircle size={15} className="spin" />
                ) : (
                  <>
                    <Download size={15} />
                    Set up
                  </>
                )}
              </button>
            </div>
            <details className="advanced-settings">
              <summary>Advanced performance settings</summary>
              <SettingRow
                title="Transcription CPU threads"
                description="Two threads is a considerate default while you're on a call."
              >
                <select
                  aria-label="Transcription CPU threads"
                  value={s.threads}
                  disabled={!!busy}
                  onChange={(e) => change({ threads: Number(e.target.value) })}
                >
                  {[1, 2, 4].map((n) => (
                    <option value={n} key={n}>
                      {n} {n === 1 ? "thread" : "threads"}
                    </option>
                  ))}
                </select>
              </SettingRow>
              <SettingRow
                title="Release speech models from memory"
                description="Models load again when your next recording starts."
              >
                <button
                  className="button secondary compact"
                  disabled={!!busy || !!status.activeMeetingId}
                  onClick={() =>
                    void run("unload", async () => {
                      await call("unload_model");
                      notify("Speech models released from memory.");
                    })
                  }
                >
                  <Unplug size={14} />
                  Release
                </button>
              </SettingRow>
            </details>
            <div className="information-note">
              <ShieldCheck size={17} />
              <div>
                <strong>Bot-free, with your permission.</strong>
                <p>
                  Allow your microphone and computer audio when your system
                  asks. Use headphones for clearer recordings and more accurate
                  speaker names.
                </p>
              </div>
            </div>
            <details className="advanced-settings technical-details">
              <summary>Technical details</summary>
              <p>
                Speech recognition uses NVIDIA Parakeet TDT v3 with INT8 models.
                On macOS, computer audio uses ScreenCaptureKit; on Windows,
                WASAPI loopback; on Linux, a PulseAudio or PipeWire monitor.
                Resource use varies by device.
              </p>
            </details>
          </>
        )}
        {tab === "Notion" && (
          <>
            <div className="section-heading">
              <Database size={18} />
              <h2>Leave the meeting with everything in place.</h2>
            </div>
            <p className="section-description">
              Send the recap, personal notes and tasks to a database you choose.
              Audio and the raw transcript stay on this computer.
            </p>
            <NotionConnection settings={s} onUpdate={onUpdate} notify={notify} />
            <fieldset className="notion-content-options">
              <legend>What to publish</legend>
              <p>Choose which meeting sections to send to your selected Notion database. The selected text is stored in Notion; the sign-in relay does not receive your notes.</p>
              <div>
                {([ ["recap", "Recap"], ["notes", "My notes"], ["tasks", "Action items"] ] as const).map(([key, label]) => (
                  <label key={key}><input type="checkbox" checked={notionSelection[key]} disabled={!!busy} onChange={e => {
                    const notionContent = {...notionSelection, [key]: e.target.checked};
                    setNotionSelection(notionContent);
                    const turnOffAutomatic = s.notionAutoSync && !Object.values(notionContent).some(Boolean);
                    void run("settings", async () => {
                      try { await onUpdate({notionContent, ...(turnOffAutomatic ? {notionAutoSync:false} : {})}); }
                      catch (e) { setNotionSelection(s.notionContent || {recap:true, notes:true, tasks:true}); throw e; }
                      if (turnOffAutomatic) notify("Automatic publishing turned off. Choose at least one section before publishing.");
                    });
                  }} />{label}</label>
                ))}
              </div>
              {s.notionContent && !Object.values(s.notionContent).some(Boolean) && <p className="hint" role="status">Choose at least one section before publishing.</p>}
            </fieldset>
            <SettingRow
              title="Automatically publish finished notes"
              description="When a meeting finishes, publish the selected sections to your database. Voice notes are excluded. Use Update notes to send later changes to an existing page."
            >
              <Toggle
                label="Automatically publish finished notes"
                checked={s.notionAutoSync}
                disabled={!!busy || !s.notionTarget || !s.hasNotionToken || (s.notionContent && !Object.values(s.notionContent).some(Boolean))}
                onChange={(v) => change({ notionAutoSync: v })}
              />
            </SettingRow>
            <div className="information-note">
              <Database size={17} />
              <div>
                <strong>Your library comes first.</strong>
                <p>
                  If Notion is unavailable, your transcript and notes remain
                  saved locally. You can publish them later from the
                  conversation.
                </p>
              </div>
            </div>
          </>
        )}
        {tab === "Privacy" && (
          <>
            <div className="section-heading">
              <ShieldCheck size={18} />
              <h2>Your conversation. Your controls.</h2>
            </div>
            <p className="section-description">
              Tuck keeps meeting data in a local SQLite database. Hosted AI and
              Notion are optional connections.
            </p>
            <BackupRestore
              meetingCount={meetingCount}
              beforeFileOperation={beforeFileOperation}
              onLibraryChanged={onLibraryChanged}
              notify={notify}
            />
            <SettingRow
              title="Keep local audio"
              description="Retain recordings on this computer for recovery. Turning this off affects future successful recordings."
            >
              <Toggle
                label="Keep local audio"
                checked={s.keepAudio}
                disabled={!!busy}
                onChange={(v) => change({ keepAudio: v })}
              />
            </SettingRow>
            <div className="privacy-table">
              <div>
                <AudioLines size={17} />
                <strong>Speech recognition</strong>
                <span>On this device</span>
              </div>
              <div>
                <Database size={17} />
                <strong>Meeting library</strong>
                <span>Local SQLite</span>
              </div>
              <div>
                <Sparkles size={17} />
                <strong>Summaries & assistant</strong>
                <span>
                  {s.provider === "local"
                    ? "Local Ollama"
                    : "OpenAI · when you ask"}
                </span>
              </div>
              <div>
                <ShieldCheck size={17} />
                <strong>API credentials</strong>
                <span>System credential store</span>
              </div>
            </div>
            <p className="privacy-copy">
              Local storage is protected by your operating system account, not
              by Tuck-specific encryption. Use device encryption and an account
              you trust. Deleting a conversation removes Tuck's local copy;
              exports and published Notion pages are separate.
            </p>
            <div className="information-note">
              <Cpu size={17} />
              <div>
                <strong>Desktop first. Ready to travel.</strong>
                <p>
                  The interface adapts to smaller screens. Native mobile audio
                  capture is not included in this build. Mobile operating
                  systems restrict recording other apps' calls.
                </p>
              </div>
            </div>
            <div className="version-line">
              <span>
                Tuck 0.1.0 · {native ? status.platform : "Browser preview"}
              </span>
              <span>Engine {status.engineVersion}</span>
            </div>
          </>
        )}
      </div>
    </section>
  );
}

function tabNavigation(e: KeyboardEvent<HTMLDivElement>) {
  if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(e.key)) return;
  const tabs = Array.from(
    e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=tab]"),
  );
  const at = tabs.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    e.key === "Home"
      ? 0
      : e.key === "End"
        ? tabs.length - 1
        : (at + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
  e.preventDefault();
  tabs[next]?.focus();
  tabs[next]?.click();
}
