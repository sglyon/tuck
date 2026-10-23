import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { call } from "../lib/bridge";
import type { Recipe, Settings, Template } from "../lib/types";
export default function SavedWriting({
  settings,
  onRefresh,
  notify,
}: {
  settings: Settings;
  onRefresh: () => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [kind, setKind] = useState<"template" | "recipe">("template");
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [instructions, setInstructions] = useState("");
  const [busy, setBusy] = useState(false);
  const items =
    kind === "template" ? settings.customTemplates : settings.recipes;
  const clear = () => {
    setId("");
    setName("");
    setInstructions("");
  };
  const select = (item: Template | Recipe) => {
    setId(item.id);
    setName(item.name);
    setInstructions("instructions" in item ? item.instructions : item.prompt);
  };
  return (
    <section className="saved-writing">
      <h2>Templates & recipes</h2>
      <p className="section-description">
        Templates shape a meeting recap. Recipes save an instruction you can run
        on notes or a recap, then review before accepting.
      </p>
      <div
        className="writing-kind"
        role="group"
        aria-label="Saved writing type"
      >
        <button
          aria-pressed={kind === "template"}
          onClick={() => {
            setKind("template");
            clear();
          }}
        >
          Recap templates
        </button>
        <button
          aria-pressed={kind === "recipe"}
          onClick={() => {
            setKind("recipe");
            clear();
          }}
        >
          Writing recipes
        </button>
      </div>
      <div className="saved-writing-list">
        {items.map((i) => (
          <button
            key={i.id}
            className={id === i.id ? "selected" : ""}
            onClick={() => select(i)}
          >
            {i.name}
          </button>
        ))}
        <button onClick={clear}>
          <Plus size={13} />
          New {kind}
        </button>
      </div>
      <form
        className="writing-form"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await call(`upsert_${kind}`, {
              [kind]: {
                ...(id ? { id } : {}),
                name,
                [kind === "template" ? "instructions" : "prompt"]: instructions,
              },
            });
            await onRefresh();
            clear();
            notify(`${kind === "template" ? "Template" : "Recipe"} saved.`);
          } catch (e) {
            notify(String(e), true);
          } finally {
            setBusy(false);
          }
        }}
      >
        <label>
          Name
          <input
            aria-label={`${kind} name`}
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={120}
          />
        </label>
        <label>
          {kind === "template"
            ? "What should the recap include?"
            : "What should Tuck do?"}
          <textarea
            aria-label={`${kind} instructions`}
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            rows={5}
            placeholder={
              kind === "template"
                ? "Focus on customer goals, objections, decisions and next steps. Cite spoken evidence. Keep unknown owners explicit."
                : "Turn these notes into a concise follow-up email. Preserve facts and flag unanswered questions."
            }
            required
          />
        </label>
        <div className="proposal-actions">
          <button
            className="button primary compact"
            disabled={busy || !name.trim() || !instructions.trim()}
          >
            Save {kind}
          </button>
          {id && (
            <button
              type="button"
              className="button secondary compact"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await call(`delete_${kind}`, { id });
                  await onRefresh();
                  clear();
                } catch (e) {
                  notify(String(e), true);
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={13} />
              Delete
            </button>
          )}
        </div>
      </form>
      <p className="quiet-note">
        Five built-in templates remain available in every meeting. Saved
        instructions stay on this device.
      </p>
    </section>
  );
}
