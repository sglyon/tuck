import type { Segment, Speaker } from "./types";

export interface SpeakerLane {
  speaker: Speaker;
  segments: Segment[];
  intervals: { start: number; end: number }[];
  seconds: number;
  share: number;
}

export function speakerLabel(id: string) {
  return id === "you" ? "You" : id.replace(/^speaker-/, "Speaker ") || "Unassigned";
}

export function buildSpeakerTimeline(segments: Segment[], speakers: Speaker[], duration: number) {
  const lanes = new Map<string, SpeakerLane>();
  let end = Number.isFinite(duration) ? Math.max(0, duration) : 0;
  for (const segment of segments) {
    if (!Number.isFinite(segment.start) || !Number.isFinite(segment.end) || !segment.text.trim()) continue;
    const start = Math.max(0, segment.start);
    if (segment.end <= start) continue;
    end = Math.max(end, segment.end);
    let lane = lanes.get(segment.speakerId);
    if (!lane) {
      lane = {
        speaker: speakers.find(speaker => speaker.id === segment.speakerId) ?? {
          id: segment.speakerId, name: speakerLabel(segment.speakerId), color: 1,
        },
        segments: [], intervals: [], seconds: 0, share: 0,
      };
      lanes.set(segment.speakerId, lane);
    }
    lane.segments.push({ ...segment, start });
  }
  for (const lane of lanes.values()) {
    lane.segments.sort((a, b) => a.start - b.start || a.end - b.end);
    for (const segment of lane.segments) {
      const previous = lane.intervals.at(-1);
      if (previous && segment.start <= previous.end) previous.end = Math.max(previous.end, segment.end);
      else lane.intervals.push({ start: segment.start, end: segment.end });
    }
    lane.seconds = lane.intervals.reduce((sum, interval) => sum + interval.end - interval.start, 0);
  }
  const speechSeconds = [...lanes.values()].reduce((sum, lane) => sum + lane.seconds, 0);
  for (const lane of lanes.values()) lane.share = lane.seconds / speechSeconds;
  return { duration: end, lanes: [...lanes.values()].sort((a, b) => b.seconds - a.seconds) };
}

// Prefer the passage under the pointer; in silence, use the closest boundary.
export function nearestPassage(segments: Segment[], time: number) {
  return segments.reduce<Segment | undefined>((nearest, segment) => {
    const distance = (s: Segment) => Math.max(s.start - time, time - s.end, 0);
    return !nearest || distance(segment) < distance(nearest) ? segment : nearest;
  }, undefined);
}
