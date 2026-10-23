import { useState } from "react";
import {
  Archive,
  ArrowRight,
  Check,
  Download,
  LoaderCircle,
  Upload,
} from "lucide-react";
import {
  call,
  chooseBackupDestination,
  chooseBackupFile,
  native,
} from "../lib/bridge";
type Preview = {
  meetingCount: number;
  restorable: number;
  skipped: number;
  audioFiles: number;
  warnings: string[];
  settingsMode: string;
  fingerprint: string;
};
type RestoreReport = {
  restored: number;
  skipped: number;
  audioFilesRestored: number;
  warnings: string[];
  settingsMode: string;
};
type BackupReport = {
  path: string;
  meetingCount: number;
  audioIncluded: boolean;
  audioFiles?: number;
  warnings: string[];
};
function count(value: number, label: string) {
  return `${value} ${label}${value === 1 ? "" : "s"}`;
}
export default function BackupRestore({
  meetingCount,
  beforeFileOperation,
  onLibraryChanged,
  notify,
}: {
  meetingCount: number;
  beforeFileOperation: () => Promise<boolean>;
  onLibraryChanged: () => Promise<void>;
  notify: (text: string, error?: boolean) => void;
}) {
  const [includeAudio, setAudio] = useState(false);
  const [busy, setBusy] = useState<"backup" | "preview" | "restore" | null>(
    null,
  );
  const [preview, setPreview] = useState<{
    path: string;
    details: Preview;
  } | null>(null);
  const [backupReport, setBackupReport] = useState<BackupReport | null>(null);
  const [restoreReport, setRestoreReport] = useState<RestoreReport | null>(
    null,
  );
  const [error, setError] = useState("");
  const perform = async (
    kind: "backup" | "preview" | "restore",
    fn: () => Promise<void>,
  ) => {
    if (busy) return;
    setBusy(kind);
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };
  const save = () =>
    void perform("backup", async () => {
      if (!(await beforeFileOperation())) return;
      const path = await chooseBackupDestination();
      if (!path) return;
      if (!(await beforeFileOperation())) return;
      setBackupReport(
        await call<BackupReport>("backup_library", { path, includeAudio }),
      );
      notify("Library backup saved.");
    });
  const inspect = () =>
    void perform("preview", async () => {
      if (!(await beforeFileOperation())) return;
      const path = await chooseBackupFile();
      if (!path) return;
      if (!(await beforeFileOperation())) return;
      setPreview(null);
      setRestoreReport(null);
      setPreview({
        path,
        details: await call<Preview>("preview_restore_library", { path }),
      });
    });
  const restore = () =>
    void perform("restore", async () => {
      if (!preview || !(await beforeFileOperation())) return;
      const result = await call<RestoreReport>("restore_library", {
        path: preview.path,
        expectedFingerprint: preview.details.fingerprint,
      });
      setRestoreReport(result);
      setPreview(null);
      await onLibraryChanged();
      notify("Backup merged. Existing meetings were kept.");
    });
  const warnings = (items: string[] | undefined) =>
    !!items?.length && (
      <div className="backup-warnings">
        <h4>Things to know</h4>
        <ul>
          {items.map((text, i) => (
            <li key={i}>{text}</li>
          ))}
        </ul>
      </div>
    );
  return (
    <section className="backup-restore">
      <div className="section-heading">
        <Archive size={18} />
        <h2>Keep a copy of your library</h2>
      </div>
      <p className="section-description">
        Save a portable file, or bring meetings back from an earlier backup.
        Existing meetings are kept exactly as they are.
      </p>
      <div className="backup-section">
        <h3>Save a backup</h3>
        <p>
          {meetingCount} {meetingCount === 1 ? "meeting" : "meetings"}, plus
          saved templates, recipes and text history. Credentials are never
          included.
        </p>
        <label className="inline-check">
          <input
            type="checkbox"
            aria-label="Include retained audio in backup"
            checked={includeAudio}
            disabled={!!busy}
            onChange={(e) => setAudio(e.target.checked)}
          />
          Include retained audio
        </label>
        <p className="quiet-note">
          {includeAudio
            ? "Any available retained recordings will be included. This can make the file much larger."
            : "Text only. Your original recordings stay on this computer."}{" "}
          Backup files contain private meeting content.
        </p>
        <button
          className="button secondary compact"
          disabled={!!busy || !native}
          onClick={save}
        >
          {busy === "backup" ? (
            <LoaderCircle className="spin" size={14} />
          ) : (
            <Download size={14} />
          )}
          Save backup…
        </button>
        {backupReport && (
          <div className="backup-report" role="status">
            <strong>
              <Check size={14} />
              Backup saved
            </strong>
            <p>
              {count(backupReport.meetingCount, "meeting")} ·{" "}
              {backupReport.audioIncluded
                ? `${count(backupReport.audioFiles ?? 0, "audio file")} included`
                : "text only"}
            </p>
            <span>{backupReport.path.split(/[\\/]/).pop()}</span>
            {warnings(backupReport.warnings)}
          </div>
        )}
      </div>
      <div className="backup-section">
        <h3>Restore from a backup</h3>
        <p>
          Choose a Tuck backup to preview its contents. Only meetings that are
          missing from this library will be added.
        </p>
        <button
          className="button secondary compact"
          disabled={!!busy || !native}
          onClick={inspect}
        >
          {busy === "preview" ? (
            <LoaderCircle className="spin" size={14} />
          ) : (
            <Upload size={14} />
          )}
          Choose backup…
        </button>
        {preview && (
          <section className="restore-preview" aria-label="Backup preview">
            <h4>{preview.path.split(/[\\/]/).pop()}</h4>
            <dl>
              <div>
                <dt>Meetings in this backup</dt>
                <dd>{preview.details.meetingCount}</dd>
              </div>
              <div>
                <dt>New meetings to add</dt>
                <dd>{preview.details.restorable}</dd>
              </div>
              <div>
                <dt>Already here · will be skipped</dt>
                <dd>{preview.details.skipped}</dd>
              </div>
              <div>
                <dt>Audio files in the archive</dt>
                <dd>{preview.details.audioFiles}</dd>
              </div>
            </dl>
            <p className="quiet-note">
              Existing meetings, transcripts and notes will not be overwritten
              or deleted. Hosted processing, automatic actions and Notion
              publishing are not activated by restore.
            </p>
            {warnings(preview.details.warnings)}
            <div className="proposal-actions">
              <button
                className="button primary compact"
                disabled={!!busy}
                onClick={restore}
              >
                {busy === "restore" ? (
                  <LoaderCircle className="spin" size={14} />
                ) : (
                  <ArrowRight size={14} />
                )}
                Merge backup
              </button>
              <button
                className="button secondary compact"
                disabled={!!busy}
                onClick={() => setPreview(null)}
              >
                Cancel
              </button>
            </div>
          </section>
        )}
        {restoreReport && (
          <div className="backup-report" role="status">
            <strong>
              <Check size={14} />
              Restore complete
            </strong>
            <p>
              {count(restoreReport.restored,"meeting")} added · {restoreReport.skipped}{" "}
              already present · {count(restoreReport.audioFilesRestored,"audio file")} restored.
            </p>
            <p className="quiet-note">
              {restoreReport.settingsMode === "saved-writing-merged"
                ? "Your current settings were kept; missing saved-writing items were added."
                : "Safe preferences were restored. Review your settings before using connected services."}
            </p>
            {warnings(restoreReport.warnings)}
          </div>
        )}
      </div>
      {!native && (
        <p className="quiet-note">
          Local backup and restore are available in the desktop app.
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error} Your current library is still here. Choose the backup again if
          its contents changed.
        </p>
      )}
    </section>
  );
}
