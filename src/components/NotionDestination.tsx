import { useState } from "react";
import { ArrowRight, LoaderCircle, Search } from "lucide-react";
import { call, native } from "../lib/bridge";
import type { Settings } from "../lib/types";
type Destination = { id: string; title: string; url: string };
type Schema = {
  id: string;
  title: string;
  properties: { name: string; type: string }[];
};
export default function NotionDestination({
  settings,
  onUpdate,
  notify,
}: {
  settings: Settings;
  onUpdate: (v: Partial<Settings>) => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [destinations, setDestinations] = useState<Destination[] | null>(null);
  const [schema, setSchema] = useState<Schema | null>(null);
  const [busy, setBusy] = useState(false);
  const [more, setMore] = useState(false);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), true);
    } finally {
      setBusy(false);
    }
  };
  const maps = [
    ["date", "Meeting date", ["date"]],
    ["folder", "Folder", ["select", "rich_text"]],
    ["tags", "Tags", ["multi_select", "rich_text"]],
    ["people", "People", ["multi_select", "rich_text"]],
    ["meetingId", "Tuck meeting ID", ["rich_text"]],
  ] as const;
  return (
    <div className="notion-destination">
      <form
        className="inline-input"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const result = await call<{
              destinations: Destination[];
              hasMore: boolean;
            }>("notion_destinations", { query });
            setDestinations(result.destinations);
            setMore(result.hasMore);
          });
        }}
      >
        <input
          aria-label="Find a Notion database"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Find a shared database…"
        />
        <button
          className="button secondary compact"
          disabled={busy || !native || !settings.hasNotionToken}
        >
          {busy ? (
            <LoaderCircle size={14} className="spin" />
          ) : (
            <Search size={14} />
          )}
          Find
        </button>
      </form>
      {destinations && (
        <div className="destination-results">
          {destinations.map((d) => (
            <button
              className={settings.notionTarget === d.id ? "selected" : ""}
              key={d.id}
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await onUpdate({
                    notionTarget: d.id,
                    notionPropertyMapping: {},
                  });
                  setSchema(
                    await call<Schema>("notion_destination", { target: d.id }),
                  );
                  notify("Notion destination saved.");
                })
              }
            >
              {d.title}
              <ArrowRight size={13} />
            </button>
          ))}
          {!destinations.length && (
            <p className="quiet-note">
              No shared databases found. Add your integration in the database's
              Connections menu, then search again.
            </p>
          )}
          {more && (
            <p className="quiet-note">
              More databases are available. Narrow your search by name.
            </p>
          )}
        </div>
      )}
      <details
        className="notion-mapping"
        onToggle={(e) => {
          if (
            e.currentTarget.open &&
            !schema &&
            settings.notionTarget &&
            native
          )
            void run(async () =>
              setSchema(await call<Schema>("notion_destination")),
            );
        }}
      >
        <summary>Map meeting details to database columns</summary>
        <p className="quiet-note">
          Optional. Choose existing compatible columns. Tuck does not create or
          rename columns.
        </p>
        {schema && (
          <>
            <strong>{schema.title}</strong>
            {maps.map(([key, label, types]) => (
              <label key={key}>
                {label}
                <select
                  aria-label={`Notion ${label.toLowerCase()} column`}
                  disabled={busy}
                  value={settings.notionPropertyMapping[key] || ""}
                  onChange={(e) =>
                    void run(() =>
                      onUpdate({
                        notionPropertyMapping: {
                          ...settings.notionPropertyMapping,
                          [key]: e.target.value,
                        },
                      }),
                    )
                  }
                >
                  <option value="">Don't map</option>
                  {schema.properties
                    .filter((p) =>
                      (types as readonly string[]).includes(p.type),
                    )
                    .map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                </select>
              </label>
            ))}
          </>
        )}
        {!schema && (
          <p className="quiet-note">
            Save a destination, then open this section to load its columns.
          </p>
        )}
      </details>
    </div>
  );
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
