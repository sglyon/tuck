import { useEffect, useRef, useState } from "react";
import {
  AudioLines,
  Check,
  ChevronRight,
  LoaderCircle,
  Pencil,
  RotateCcw,
  X,
} from "lucide-react";
import { call, native } from "../lib/bridge";
import { clock } from "../lib/format";
import type {
  Meeting,
  RetainedAudioFile,
  Segment,
  TranscriptDraft,
} from "../lib/types";
export function fileSize(bytes: number) {
  return bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
type Shared = {
  meeting: Meeting;
  merge: (m: Meeting) => void;
  notify: (text: string, error?: boolean) => void;
};
export function TranscriptCorrection({
  meeting,
  segment,
  draft,
  onDraft,
  merge,
  notify,
}: Shared & {
  segment: Segment;
  draft?: TranscriptDraft;
  onDraft: (draft: TranscriptDraft | null) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inactive = !["recording", "paused", "processing"].includes(
    meeting.status,
  );
  const action = async (method: "correct_transcript" | "reset_transcript") => {
    if (
      method === "correct_transcript" &&
      (!draft ||
        draft.meetingId !== meeting.id ||
        draft.segmentId !== segment.id)
    ) {
      setError("Reopen this passage before saving its correction.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      merge(
        await call<Meeting>(method, {
          meetingId: meeting.id,
          segmentId: segment.id,
          baseText:
            method === "correct_transcript" ? draft?.baseText : segment.text,
          ...(method === "correct_transcript" ? { text: draft?.text } : {}),
        }),
      );
      onDraft(null);
      notify(
        method === "correct_transcript"
          ? "Transcript correction saved. The original words remain available."
          : "Original transcript restored.",
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="transcript-correction">
      {!draft && (
        <div className="correction-actions">
          {segment.edited && (
            <span className="corrected-label">
              <Check size={12} />
              Corrected by you
            </span>
          )}
          <button
            className="text-link"
            disabled={!inactive}
            title={
              !inactive
                ? "Finish recording before correcting this passage"
                : undefined
            }
            onClick={() => {
              setError("");
              onDraft({
                meetingId: meeting.id,
                segmentId: segment.id,
                baseText: segment.text,
                text: segment.text,
              });
            }}
          >
            <Pencil size={12} />
            Correct words
          </button>
          {segment.edited && (
            <details className="original-transcript">
              <summary>Original transcript</summary>
              <p>{segment.originalText}</p>
              <button
                className="text-link"
                disabled={busy || !inactive}
                onClick={() => void action("reset_transcript")}
              >
                <RotateCcw size={12} />
                Restore original
              </button>
            </details>
          )}
        </div>
      )}
      {draft && (
        <div className="correction-editor">
          <label>
            Correct this passage
            <textarea
              aria-label={`Correction at ${clock(segment.start)}`}
              rows={3}
              value={draft.text}
              maxLength={32000}
              disabled={busy}
              onChange={(e) => onDraft({ ...draft, text: e.target.value })}
            />
          </label>
          <p className="quiet-note">
            This updates the words used by search, exports and AI. The original
            transcript is kept. Existing recaps are unchanged.
          </p>
          <details className="original-transcript">
            <summary>Original speech recognition</summary>
            <p>{segment.originalText ?? segment.text}</p>
          </details>
          <div className="proposal-actions">
            <button
              className="button primary compact"
              disabled={
                busy ||
                !draft.text.trim() ||
                draft.text === draft.baseText ||
                !inactive
              }
              onClick={() => void action("correct_transcript")}
            >
              {busy ? (
                <LoaderCircle className="spin" size={13} />
              ) : (
                <Check size={13} />
              )}
              Save correction
            </button>
            <button
              className="button secondary compact"
              disabled={busy}
              onClick={() => {
                setError("");
                onDraft(null);
              }}
            >
              <X size={13} />
              Cancel
            </button>
          </div>
        </div>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error} Your correction is still here. Cancel and reopen it to use the
          latest passage.
        </p>
      )}
    </div>
  );
}
export function RetainedAudio({
  meeting,
  merge,
  notify,
  beforeOperation,
  onRecovered,
  onSeekReady,
  openRequested = false,
}: Shared & {
  beforeOperation: () => Promise<boolean>;
  onRecovered: (m: Meeting) => void;
  onSeekReady: (seek: ((seconds: number) => void) | null) => void;
  openRequested?: boolean;
}) {
  const [open, setOpen] = useState(openRequested);
  const [files, setFiles] = useState<RetainedAudioFile[] | null>(null);
  const [audioId, setAudioId] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [refining, setRefining] = useState(false);
  const [error, setError] = useState("");
  const [position, setPosition] = useState<number | null>(null);
  const audio = useRef<HTMLAudioElement>(null);
  const request = useRef(0);
  const desiredTime = useRef<number | null>(null);
  const inactive = !["recording", "paused", "processing"].includes(
    meeting.status,
  );
  useEffect(() => {
    if (openRequested) setOpen(true);
  }, [openRequested]);
  useEffect(() => {
    if (!open || !inactive || files !== null) return;
    let cancelled = false;
    setBusy(true);
    setError("");
    void call<RetainedAudioFile[]>("meeting_audio", { meetingId: meeting.id })
      .then((result) => {
        if (!cancelled) {
          setFiles(result);
          setAudioId(result[0]?.id || "");
        }
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, meeting.id, inactive]);
  useEffect(() => {
    const seek = (seconds: number) => {
      if (!url) return;
      desiredTime.current = seconds;
      if (audio.current && audio.current.readyState >= 1) {
        audio.current.currentTime = Math.max(0, seconds);
        desiredTime.current = null;
      }
    };
    onSeekReady(open && url ? seek : null);
    return () => onSeekReady(null);
  }, [url, open, onSeekReady]);
  const load = async () => {
    const generation = ++request.current;
    setBusy(true);
    setError("");
    setUrl("");
    setPosition(null);
    desiredTime.current = null;
    try {
      const result = await call<{ url: string }>("audio_source", {
        meetingId: meeting.id,
        audioId,
      });
      if (generation === request.current) setUrl(result.url);
    } catch (e) {
      if (generation === request.current)
        setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (generation === request.current) setBusy(false);
    }
  };
  const recover = async (refine = false) => {
    setRefining(refine);
    setBusy(true);
    setError("");
    try {
      if (!(await beforeOperation())) return;
      const recovered = await call<Meeting>(refine ? "refine_audio" : "recover_audio", {
        meetingId: meeting.id,
        audioId,
      });
      merge(recovered);
      onRecovered(recovered);
      notify(
        refine ? "Refined transcript saved as a separate meeting." : "Saved audio recovered into a separate meeting. The original is unchanged.",
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setRefining(false);
    }
  };
  return (
    <details
      className="retained-audio"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary>
        <AudioLines size={15} />
        Saved audio
        <ChevronRight size={13} />
      </summary>
      <div>
        {!inactive ? (
          <p className="quiet-note">
            Saved audio is available after recording finishes.
          </p>
        ) : (
          <>
            <p className="quiet-note">
              Listen to saved audio or create a separate transcript. Refinement
              combines all saved tracks to reduce echoes and improve speaker
              grouping. Audio stays on this Mac.
            </p>
            {files?.some((file) => file.id === "import.wav") && (
              <p className="quiet-note">
                For imported compressed audio or WAV that needs conversion, the
                retained copy is 16 kHz mono PCM WAV. Your original file is
                unchanged.
              </p>
            )}
            {busy && (
              <p className="quiet-note" role="status">
                <LoaderCircle className="spin" size={13} />
                {refining ? "Refining transcript and speakers…" : files ? "Working with saved audio…" : "Checking saved audio…"}
              </p>
            )}
            {files?.length === 0 && (
              <p className="quiet-note">
                {native
                  ? "No retained audio is available for this meeting."
                  : "This illustrative browser example has no recording. Audio playback and recovery use the desktop app."}
              </p>
            )}
            {!!files?.length && (
              <>
                <label className="saved-audio-source">
                  Recording source
                  <select
                    aria-label="Saved audio source"
                    disabled={busy}
                    value={audioId}
                    onChange={(e) => {
                      request.current++;
                      setAudioId(e.target.value);
                      setUrl("");
                      setPosition(null);
                      desiredTime.current = null;
                      setError("");
                    }}
                  >
                    {files.map((f) => (
                      <option value={f.id} key={f.id}>
                        {f.name} · {fileSize(f.bytes)}
                        {f.repairable ? " · recovery available" : ""}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="saved-audio-actions">
                  <button className="button secondary compact" disabled={busy || meeting.isExample} onClick={() => void recover(true)}>
                    <RotateCcw size={14} />Refine transcript
                  </button>
                  <button
                    className="button secondary compact"
                    disabled={busy || !audioId}
                    onClick={() => void load()}
                  >
                    <AudioLines size={14} />
                    Load audio
                  </button>
                  <button
                    className="button subtle compact"
                    disabled={busy || !audioId || meeting.isExample}
                    onClick={() => void recover()}
                  >
                    <RotateCcw size={14} />
                    Recover saved audio
                  </button>
                </div>
                <p className="quiet-note">
                  Recovery creates a new meeting and keeps this meeting's notes,
                  transcript and original recording.
                </p>
                {url && (
                  <>
                    <audio
                      key={url}
                      ref={audio}
                      controls
                      preload="metadata"
                      src={url}
                      aria-label="Retained meeting audio"
                      onLoadedMetadata={() => {
                        if (audio.current && desiredTime.current !== null) {
                          audio.current.currentTime = Math.max(
                            0,
                            desiredTime.current,
                          );
                          desiredTime.current = null;
                        }
                      }}
                      onSeeked={() => {
                        if (audio.current)
                          setPosition(audio.current.currentTime);
                      }}
                      onError={() =>
                        setError(
                          "The saved audio could not be played. Your transcript is safe. Try recovering the recording instead.",
                        )
                      }
                    />
                    <p className="quiet-note">
                      {position !== null
                        ? `Positioned at ${clock(position)}. Press play when you're ready.`
                        : "Press play when you're ready. Transcript timestamps can position this loaded recording."}
                    </p>
                  </>
                )}
              </>
            )}
            {error && (
              <p className="inline-error" role="alert">
                {error}
              </p>
            )}
          </>
        )}
      </div>
    </details>
  );
}
