import { useEffect, useState } from "react";
import { ArrowRight, Check, Database, LoaderCircle, Search } from "lucide-react";
import { call, native, openExternal } from "../lib/bridge";
import type { Settings } from "../lib/types";

type Destination = { id: string; title: string; url: string };
type Schema = Destination & { properties: { name: string; type: string }[]; setupWarning?: string };
export default function NotionDestination({ settings, onUpdate, notify }: {
  settings: Settings;
  onUpdate: (v: Partial<Settings>) => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [destinations, setDestinations] = useState<Destination[] | null>(null);
  const [schema, setSchema] = useState<Schema | null>(null);
  const [schemaTarget, setSchemaTarget] = useState("");
  const [busy, setBusy] = useState("");
  const [more, setMore] = useState(false);
  const [creating, setCreating] = useState(false);
  const [parentQuery, setParentQuery] = useState("");
  const [parents, setParents] = useState<Destination[] | null>(null);
  const [parentsMore, setParentsMore] = useState(false);
  const [parent, setParent] = useState<Destination | null>(null);
  const [setupError, setSetupError] = useState("");
  const [refreshFailed, setRefreshFailed] = useState(false);
  const setup = settings.notionSetup;
  const uncertain = refreshFailed || setup?.status === "uncertain" || setup?.status === "creating";
  const setupParent = uncertain ? setup?.parent || parent?.id : parent?.id;
  const ready = setup?.status === "ready" && setup.destination?.id === settings.notionTarget ? setup.destination : null;
  const [existingOpen, setExistingOpen] = useState(false);
  const [destinationError, setDestinationError] = useState("");
  useEffect(() => {
    setSchema(null);setSchemaTarget("");setDestinationError("");
    if (!settings.notionTarget || ready || !native) return;
    let cancelled = false;
    void call<Schema>("notion_destination", {target: settings.notionTarget}).then(result => {
      if (!cancelled) {setSchema(result);setSchemaTarget(settings.notionTarget);setDestinationError("");}
    }).catch(e => {if (!cancelled) setDestinationError(e instanceof Error ? e.message : String(e));});
    return () => {cancelled = true;};
  }, [settings.notionTarget, ready?.id]);
  const verifiedSchema = schemaTarget === settings.notionTarget ? schema : null;
  const run = async (name: string, fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(name);
    try { await fn(); }
    catch (e) { notify(e instanceof Error ? e.message : String(e), true); }
    finally { setBusy(""); }
  };
  const findParents = async (query: string) => {
    const result = await call<{ pages: Destination[]; hasMore: boolean }>("notion_parent_pages", { query });
    setParents(result.pages);
    setParentsMore(result.hasMore);
  };
  const create = async (parentId = setupParent, mode: "create" | "finish" = "create") => {
    if (busy || !parentId) return;
    setBusy(mode); setSetupError("");
    try {
      const result = await call<{ destination: Schema; settings: Settings }>(mode === "finish" ? "notion_finish_database" : "notion_create_database", { parent: parentId });
      setSchema(result.destination);setSchemaTarget(result.settings.notionTarget);
      // Native setup already saved the destination and mapping atomically. Refresh,
      // rather than saving an older snapshot over a concurrent settings change.
      await onUpdate({});
      setCreating(false);setRefreshFailed(false);
      notify(result.destination.setupWarning || "Tuck meetings is ready. Your publishing preferences are unchanged.");
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setSetupError(message);
      // Refresh the durable native journal. Repeating an unresolved setup only
      // checks for its database; the native boundary prevents duplicate creates.
      try { await onUpdate({}); } catch { setRefreshFailed(true); }
    } finally { setBusy(""); }
  };
  const maps = [
    ["date", "Meeting date", ["date"]],
    ["folder", "Folder", ["select", "rich_text"]],
    ["tags", "Tags", ["multi_select", "rich_text"]],
    ["people", "People", ["multi_select", "rich_text"]],
    ["meetingId", "Tuck meeting ID", ["rich_text"]],
  ] as const;
  return <div className="notion-destination">
    {ready && !creating && <div className="notion-template-ready">
      <Check size={16} />
      <div>
        <strong>{ready.title}</strong>
        <p>Your meeting database is ready. Meeting details are mapped for you.</p>
        {setup?.message && <p className="quiet-note" role="status">{setup.message}</p>}
        {setupError && <p className="quiet-note" role="alert">{setupError}</p>}
        <div className="notion-template-actions">
          {setup?.message && <button className="button secondary compact" disabled={!!busy || !setup.parent} onClick={() => void create(setup.parent, "finish")}>
            {busy === "finish" ? <><LoaderCircle size={14} className="spin" />Finishing setup…</> : "Finish database setup"}
          </button>}
          <a className="text-link" href={ready.url} onClick={e => {e.preventDefault();void openExternal(ready.url);}}>Open database in Notion<ArrowRight size={13} /></a>
        </div>
      </div>
    </div>}
    {settings.notionTarget && !ready && <div className="notion-template-ready"><Check size={16} /><div><strong>{verifiedSchema?.title || "Database selected"}</strong><p>{destinationError || (verifiedSchema ? "Meeting notes will be published to this database." : "Loading the selected database…")}</p></div></div>}
    {!creating && !uncertain && !ready && <div className="notion-template-intro">
      <p>A ready-to-use meeting database, with consistent details and space for your recap, notes and action items.</p>
      <button className={`button ${settings.notionTarget ? "secondary" : "primary"}`} disabled={!!busy || !native || !settings.hasNotionToken} onClick={() => {
        setCreating(true);
        if (!parents) void run("parents", () => findParents(parentQuery));
      }}><Database size={15} />Create my meeting database</button>
    </div>}
    {(creating || uncertain) && <section className="notion-template-setup" aria-label="Create a Tuck meeting database">
      <h4>{uncertain ? "Check your database setup" : "Where should Tuck meetings live?"}</h4>
      <p className="quiet-note">{uncertain ? "Setup has not been confirmed. Tuck will look for the database from this attempt before doing anything else." : "Choose a page you shared with Tuck. We’ll create a database inside it."}</p>
      {!uncertain && <form className="inline-input" onSubmit={e => {e.preventDefault(); void run("parents", () => findParents(parentQuery));}}>
        <input aria-label="Find a Notion parent page" value={parentQuery} onChange={e => setParentQuery(e.target.value)} placeholder="Find a shared page…" disabled={busy === "create"} />
        <button className="button secondary compact" disabled={!!busy}>{busy === "parents" ? <LoaderCircle size={14} className="spin" /> : <Search size={14} />}Find pages</button>
      </form>}
      {busy === "parents" && <p className="quiet-note" role="status">Finding pages shared with Tuck…</p>}
      {!uncertain && parents && <div className="destination-results notion-parent-results" aria-label="Shared Notion pages">
        {parents.map(p => <button key={p.id} type="button" disabled={!!busy || uncertain} aria-pressed={parent?.id === p.id} className={parent?.id === p.id ? "selected" : ""} onClick={() => setParent(p)}><span>{p.title}</span>{parent?.id === p.id ? <Check size={15} /> : <ArrowRight size={13} />}</button>)}
        {!parents.length && <p className="quiet-note">No shared pages found. In Notion, open the page you want to use and add Tuck from its Connections menu, then find pages again.</p>}
        {parentsMore && <p className="quiet-note">More pages are available. Search by name to find yours.</p>}
      </div>}
      <div className="notion-template-preview">
        <strong>Tuck meetings</strong>
        <p>Meeting name · Meeting date · Folder · Tags · People</p>
        <p>Recap, my notes and action items in each published meeting. A Tuck meeting ID keeps updates connected.</p>
      </div>
      {(setupError || setup?.message) && <div className="connection-error" role="alert"><p>{setupError || setup?.message}</p>{uncertain && <p>Check Notion for “Tuck meetings” before creating anything else. You can also find and select it below if it was created.</p>}<button className="text-link" onClick={() => {setExistingOpen(true); void run("existing", async () => { const result = await call<{destinations: Destination[];hasMore:boolean}>("notion_destinations", {query:"Tuck meetings"});setQuery("Tuck meetings");setDestinations(result.destinations);setMore(result.hasMore);});}} disabled={!!busy}>Find existing database</button></div>}
      <div className="notion-template-actions">
        <button className="button primary" disabled={!!busy || !setupParent} onClick={() => void create()}>{busy === "create" ? <><LoaderCircle size={15} className="spin" />{uncertain ? "Checking for database…" : "Creating database…"}</> : uncertain ? "Check for my database" : parent ? `Create in ${parent.title}` : "Choose a page to continue"}</button>
        {!uncertain && <button className="button secondary" disabled={!!busy} onClick={() => setCreating(false)}>Cancel</button>}
      </div>
      <p className="quiet-note">No meetings are uploaded during setup. Automatic publishing stays under your control.</p>
    </section>}
    <details className="notion-existing" open={existingOpen} onToggle={e => setExistingOpen(e.currentTarget.open)}>
      <summary>Use an existing database</summary>
      <form className="inline-input" onSubmit={e => {e.preventDefault();void run("existing", async () => {const result = await call<{destinations: Destination[];hasMore: boolean}>("notion_destinations", {query});setDestinations(result.destinations);setMore(result.hasMore);});}}>
        <input aria-label="Find a Notion database" value={query} onChange={e => setQuery(e.target.value)} placeholder="Find a shared database…" />
        <button className="button secondary compact" disabled={!!busy || !native || !settings.hasNotionToken}>{busy === "existing" ? <LoaderCircle size={14} className="spin" /> : <Search size={14} />}Find</button>
      </form>
      {destinations && <div className="destination-results">
        {destinations.map(d => <button className={settings.notionTarget === d.id ? "selected" : ""} key={d.id} disabled={!!busy} onClick={() => void run("select", async () => {
          await onUpdate({notionTarget: d.id, notionPropertyMapping:{}});
          const selectedSchema = await call<Schema>("notion_destination", {target:d.id});
          setSchema(selectedSchema);setSchemaTarget(d.id);
          setCreating(false);setSetupError("");
          notify("Notion destination saved.");
        })}><span>{d.title}</span><ArrowRight size={13} /></button>)}
        {!destinations.length && <p className="quiet-note">No shared databases found. Add Tuck in the database’s Connections menu, then search again.</p>}
        {more && <p className="quiet-note">More databases are available. Narrow your search by name.</p>}
      </div>}
    </details>
    {settings.notionTarget && <details className="notion-mapping" onToggle={e => {if(e.currentTarget.open && !verifiedSchema && native) void run("schema", async () => {setSchema(await call<Schema>("notion_destination", {target:settings.notionTarget}));setSchemaTarget(settings.notionTarget);setDestinationError("");});}}>
      <summary>Map meeting details to database columns</summary>
      <p className="quiet-note">Tuck’s template maps these details for you. For another database, choose compatible existing columns.</p>
      {verifiedSchema && <><strong>{verifiedSchema.title}</strong>{maps.map(([key,label,types]) => <label key={key}>{label}<select aria-label={`Notion ${label.toLowerCase()} column`} disabled={!!busy} value={settings.notionPropertyMapping[key] || ""} onChange={e => void run("mapping", () => onUpdate({notionPropertyMapping:{...settings.notionPropertyMapping,[key]:e.target.value}}))}><option value="">Don’t map</option>{verifiedSchema.properties.filter(p => (types as readonly string[]).includes(p.type)).map(p => <option key={p.name} value={p.name}>{p.name}</option>)}</select></label>)}</>}
      {destinationError && <p className="quiet-note" role="alert">{destinationError} Close and reopen this section to check the current database’s columns.</p>}
      {busy === "schema" && <p className="quiet-note" role="status">Loading database columns…</p>}
    </details>}
  </div>;
}

export function NotionRecovery({
  meetingId,
  onRecovered,
  notify,
}: {
  meetingId: string;
  onRecovered: (m: import("../lib/types").Meeting) => void;
  notify: (text: string, error?: boolean) => void;
}) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <details className="notion-recovery">
      <summary>Link a page already created in Notion</summary>
      <p className="quiet-note">
        Check your destination database first. Paste the existing page link;
        Tuck verifies its meeting marker before connecting it. This does not
        create another page.
      </p>
      <form
        className="inline-input"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            onRecovered(
              await call("recover_notion", { meetingId, url: url.trim() }),
            );
            setUrl("");
            notify(
              "Existing Notion page linked. You can now update its notes.",
            );
          } catch (e) {
            notify(e instanceof Error ? e.message : String(e), true);
          } finally {
            setBusy(false);
          }
        }}
      >
        <input
          type="url"
          aria-label="Existing Notion page link"
          placeholder="https://www.notion.so/…"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          required
        />
        <button
          className="button secondary compact"
          disabled={busy || !url.trim()}
        >
          {busy ? "Checking…" : "Link existing page"}
        </button>
      </form>
    </details>
  );
}
