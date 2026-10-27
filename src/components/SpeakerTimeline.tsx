import {
  useEffect, useId, useMemo, useState,
  type MouseEvent, type KeyboardEvent,
} from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { clock } from "../lib/format";
import {
  buildSpeakerTimeline, nearestPassage, speakerLabel, type SpeakerLane,
} from "../lib/speakerTimeline";
import type { Meeting, Segment } from "../lib/types";

export default function SpeakerTimeline({
  meeting, elapsed, onPositionReady, selectedId, onJump,
}: {
  meeting: Meeting;
  elapsed: number;
  onPositionReady: (update: ((time: number | null) => void) | null) => void;
  selectedId: string | null;
  onJump: (time: number, segmentId?: string) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const [position, setPosition] = useState<number | null>(null);
  useEffect(() => {
    onPositionReady(setPosition);
    return () => onPositionReady(null);
  }, [onPositionReady]);
  const helpId = useId();
  const lanesId = useId();
  const timeline = useMemo(
    () => buildSpeakerTimeline(meeting.segments, meeting.speakers, Math.max(meeting.duration, elapsed)),
    [meeting.segments, meeting.speakers, meeting.duration, elapsed],
  );
  if (!timeline.lanes.length) return null;
  const selected = meeting.segments.find(segment => segment.id === selectedId);
  const time = Math.min(timeline.duration, Math.max(0, position ?? selected?.start ?? 0));
  const navigate = (segment: Segment | undefined) => {
    if (segment) onJump(segment.start, segment.id);
  };
  const clickLane = (event: MouseEvent<HTMLButtonElement>, lane: SpeakerLane) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const target = event.detail === 0
      ? time
      : (event.clientX - bounds.left) / bounds.width * timeline.duration;
    navigate(nearestPassage(lane.segments, target));
  };
  const keyLane = (event: KeyboardEvent<HTMLButtonElement>, lane: SpeakerLane) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const index = lane.segments.findIndex(segment => segment.id === selectedId);
    if (event.key === "Home") navigate(lane.segments[0]);
    else if (event.key === "End") navigate(lane.segments.at(-1));
    else if (index >= 0) {
      const next = index + (event.key === "ArrowRight" ? 1 : -1);
      navigate(lane.segments[Math.max(0, Math.min(lane.segments.length - 1, next))]);
    }
    else navigate(event.key === "ArrowRight"
      ? lane.segments.find(segment => segment.start >= time) ?? lane.segments.at(-1)
      : [...lane.segments].reverse().find(segment => segment.start <= time) ?? lane.segments[0]);
  };
  return (
    <section className="speaker-timeline" aria-label="Speaker timeline">
      <div className="speaker-timeline-heading">
        <button
          className="timeline-disclosure"
          aria-expanded={expanded}
          aria-controls={lanesId}
          onClick={() => setExpanded(value => !value)}
        >
          {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
          Speaker timeline
        </button>
        <span className="timeline-position">
          {clock(time)} <span>/ {clock(timeline.duration)}</span>
        </span>
      </div>
      <div id={lanesId} hidden={!expanded}>
        <p id={helpId} className="visually-hidden">
          Click a lane to jump to the nearest passage. Left and Right arrow keys
          move between turns. Home and End jump to the first and last turn.
        </p>
        <p className="visually-hidden" role="status">
          {selected ? `${meeting.speakers.find(speaker => speaker.id === selected.speakerId)?.name ?? speakerLabel(selected.speakerId)} at ${clock(selected.start)}: ${selected.text}` : ""}
        </p>
        <div className="timeline-axis" aria-hidden="true">
          <span>00:00</span><span>{clock(timeline.duration / 2)}</span><span>{clock(timeline.duration)}</span>
        </div>
        <div className="speaker-timeline-lanes">
          {timeline.lanes.map(lane => (
            <div key={lane.speaker.id} className={`speaker-timeline-lane speaker-${((lane.speaker.color % 6) + 6) % 6}`}>
              <div className="timeline-lane-heading">
                <span className="timeline-speaker">
                  <span className="timeline-speaker-dot" />{lane.speaker.name}
                </span>
                <span className="timeline-share" title={`${clock(lane.seconds)} of transcribed speech`}>
                  {lane.share < 0.01 ? "<1" : Math.round(lane.share * 100)}%
                </span>
              </div>
              <button
                className="timeline-track"
                aria-label={`Explore ${lane.speaker.name}'s speech`}
                aria-describedby={helpId}
                onClick={event => clickLane(event, lane)}
                onKeyDown={event => keyLane(event, lane)}
              >
                <svg viewBox={`0 0 ${timeline.duration} 8`} preserveAspectRatio="none" aria-hidden="true">
                  {lane.intervals.map(interval => (
                    <rect
                      key={interval.start} x={interval.start} y={0}
                      width={interval.end - interval.start} height={8}
                      rx={Math.min(1, (interval.end - interval.start) / 2)}
                    />
                  ))}
                </svg>
                {selected?.speakerId === lane.speaker.id && (
                  <span className="timeline-selection" style={{
                    left: `${selected.start / timeline.duration * 100}%`,
                    width: `${(selected.end - selected.start) / timeline.duration * 100}%`,
                  }} />
                )}
                <span className="timeline-playhead" style={{ left: `${time / timeline.duration * 100}%` }} />
              </button>
            </div>
          ))}
        </div>
        <p className="timeline-share-note">
          Click a lane to jump · share of transcribed speech.
          <br />Speaker separation is provisional.
        </p>
      </div>
    </section>
  );
}
