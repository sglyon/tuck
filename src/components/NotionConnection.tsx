import { useEffect, useRef, useState } from "react";
import { ArrowUpRight, Check, ExternalLink, LoaderCircle } from "lucide-react";
import { call, native, openExternal } from "../lib/bridge";
import type { Settings } from "../lib/types";
import NotionDestination from "./NotionDestination";

type AccountStatus = {
  configured: boolean;
  connected: boolean;
  connecting: boolean;
  mode: "oauth" | "manual" | null;
  workspaceName: string | null;
  workspaceId: string | null;
  revocationWarning?: string;
  message?: string;
};
export default function NotionConnection({settings, onUpdate, notify}: {
  settings: Settings;
  onUpdate: (v: Partial<Settings>) => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [account, setAccount] = useState<AccountStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState("");
  const [token, setToken] = useState("");
  const [target, setTarget] = useState(settings.notionTarget);
  const generation = useRef(0);
  useEffect(() => setTarget(settings.notionTarget), [settings.notionTarget]);
  const refresh = async () => {
    setLoading(true);
    try {
      const result = await call<AccountStatus>("notion_account_status");
      setAccount(result);
      if (result.connected !== settings.hasNotionToken) await onUpdate({});
      setError("");
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setLoading(false); }
  };
  useEffect(() => { if (native) void refresh(); else setLoading(false); return () => { generation.current++; }; }, []);
  useEffect(() => {
    if (!account?.connecting || busy === "connect") return;
    let cancelled = false;
    const timer = setInterval(() => {
      void call<AccountStatus>("notion_account_status").then(async result => {
        if (cancelled) return;
        setAccount(result);
        if (!result.connecting) await onUpdate({});
      }).catch(e => { if (!cancelled) setError(e instanceof Error ? e.message : String(e)); });
    }, 1500);
    return () => { cancelled = true; clearInterval(timer); };
  }, [account?.connecting, busy]);
  const connect = async () => {
    const request = ++generation.current;
    setBusy("connect"); setError("");
    try {
      const result = await call<AccountStatus>("notion_connect");
      if (request !== generation.current) return;
      setAccount(result);
      await onUpdate({});
      if (result.connected) notify("Notion connected. Choose a destination for your meeting notes.");
    } catch (e) { if (request === generation.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (request === generation.current) setBusy(""); }
  };
  const cancel = async () => {
    ++generation.current; setBusy("cancel");
    try { setAccount(await call<AccountStatus>("notion_cancel_connect")); setError(""); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(""); }
  };
  const disconnect = async () => {
    setBusy("disconnect"); setError("");
    try {
      const result = await call<AccountStatus>("notion_disconnect");
      setAccount(result);
      await onUpdate({});
      notify(result.revocationWarning || "Notion disconnected. Your local notes and existing Notion pages are kept.", !!result.revocationWarning);
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(""); }
  };
  const connected = account ? account.connected : settings.hasNotionToken;
  const connecting = busy === "connect" || account?.connecting;
  return <div className="notion-connection">
    <div className="notion-account">
      <div>
        <h3>{connecting ? "Finish connecting in your browser" : connected ? (account?.workspaceName || (account?.mode === "manual" ? "Manual integration connected" : "Notion connected")) : "Connect your Notion workspace"}</h3>
        <p>{connecting ? "Choose the pages Tuck may access, then return here. Your current notes stay open." : connected ? "Choose where meeting recaps, notes and tasks are published." : "Sign in to Notion and choose the pages Tuck may access."}</p>
      </div>
      {connecting ? <button className="button secondary" onClick={() => void cancel()} disabled={busy === "cancel"}>{busy === "cancel" ? "Cancelling…" : "Cancel connection"}</button> : connected ? <button className="button secondary" disabled={!!busy} onClick={() => void disconnect()}>{busy === "disconnect" ? "Disconnecting…" : "Disconnect"}</button> : <button className="button primary" disabled={!native || loading || !account?.configured || !!busy} onClick={() => void connect()}>{loading ? <LoaderCircle size={15} className="spin" /> : <ExternalLink size={15} />}Connect with Notion</button>}
    </div>
    {!loading && !connected && !connecting && <p className="quiet-note">{!native ? "Connect with Notion is available in the desktop app." : account && !account.configured ? "Notion sign-in is not configured in this build. A manual integration is available under Advanced connection." : "Your system browser opens for secure sign-in. Tuck stores the connection in your system credential store."}</p>}
    {error && <div className="connection-error" role="alert"><p>{error}</p><button className="text-link" disabled={!!busy} onClick={() => void refresh()}>Check connection again</button></div>}
    {connected && <section className="notion-target-section"><h3>Choose the destination</h3><NotionDestination settings={settings} onUpdate={onUpdate} notify={notify} key={`${settings.notionTarget}:${account?.workspaceId || account?.mode || "manual"}`} />{settings.notionTarget && <p className="quiet-note"><Check size={12} /> Destination saved. Automatic publishing stays under your control below.</p>}</section>}
    <details className="advanced-settings notion-advanced"><summary>Advanced connection</summary>
      <h3>Use a manual integration token</h3><p>Create an internal integration and share your destination database with it. Saving a token replaces the current connection.</p>
      <a className="text-link" href="https://www.notion.so/profile/integrations" onClick={e => {e.preventDefault(); void openExternal("https://www.notion.so/profile/integrations");}}>Open Notion integrations <ArrowUpRight size={13} /></a>
      <form className="inline-input" onSubmit={async e => {
        e.preventDefault(); if (busy || !native || !token.trim()) return; setBusy("token"); setError("");
        try { await call("save_secret", {kind: "notion", value: token.trim()}); setToken(""); await onUpdate({}); await refresh(); notify("Notion integration saved in your system credential store."); }
        catch (e) { setError(e instanceof Error ? e.message : String(e)); }
        finally { setBusy(""); }
      }}><label className="sr-only" htmlFor="notion-token">Integration token</label><input id="notion-token" type="password" value={token} onChange={e => setToken(e.target.value)} placeholder={settings.hasNotionToken ? "Enter a replacement token" : "ntn_…"} autoComplete="off" spellCheck={false}/><button className="button secondary" disabled={!native || !!busy || !token.trim()}>Save token</button></form>
      <label className="field-label" htmlFor="notion-target">Database URL or data source ID</label><div className="inline-input"><input id="notion-target" value={target} onChange={e => setTarget(e.target.value)} placeholder="https://notion.so/your-database…" spellCheck={false}/><button className="button secondary" disabled={!native || !!busy || !connected || !target.trim() || target === settings.notionTarget} onClick={async () => {
        setBusy("target"); try {await onUpdate({notionTarget: target.trim(), notionPropertyMapping: {}});} catch(e) {setError(e instanceof Error ? e.message : String(e));} finally {setBusy("");}
      }}>Save</button></div>
    </details>
  </div>;
}
