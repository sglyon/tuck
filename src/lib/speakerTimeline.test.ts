import { describe, expect, it } from "vitest";
import { buildSpeakerTimeline, nearestPassage } from "./speakerTimeline";
import type { Segment } from "./types";

const segment = (id: string, speakerId: string, start: number, end: number): Segment => ({
  id, meetingId: "meeting", speakerId, start, end, text: id, source: "system",
});

describe("speaker timeline", () => {
  it("measures speech rather than silence and counts overlapping same-speaker intervals once", () => {
    const result = buildSpeakerTimeline([
      segment("later", "a", 8, 15), segment("first", "a", 0, 10),
      segment("overlap", "b", 5, 10), segment("last", "b", 25, 30),
    ], [{ id: "a", name: "Ada", color: 2 }], 60);
    expect(result.duration).toBe(60);
    expect(result.lanes.map(lane => [lane.speaker.name, lane.seconds, lane.share])).toEqual([
      ["Ada", 15, 0.6], ["b", 10, 0.4],
    ]);
    expect(result.lanes[0].intervals).toEqual([{ start: 0, end: 15 }]);
    expect(result.lanes[0].segments.map(s => s.id)).toEqual(["first", "later"]);
  });

  it("keeps unknown labels, clamps negative starts, and uses segment extent when duration lags live speech", () => {
    const result = buildSpeakerTimeline([
      segment("unassigned", "", -2, 3), segment("new", "speaker-4", 4, 12),
      segment("zero", "ignored", 1, 1), segment("reversed", "ignored", 10, 5),
      segment("invalid", "ignored", NaN, 8), segment("infinite", "ignored", 0, Infinity),
      { ...segment("blank", "ignored", 0, 100), text: "  " },
    ], [], 5);
    expect(result.duration).toBe(12);
    expect(result.lanes.map(lane => lane.speaker.name)).toEqual(["Speaker 4", "Unassigned"]);
    expect(result.lanes[1].segments[0].start).toBe(0);
    expect(buildSpeakerTimeline([], [], NaN)).toEqual({ duration: 0, lanes: [] });
  });

  it("chooses speech under the pointer and the closest passage across gaps", () => {
    const passages = [segment("one", "a", 2, 5), segment("two", "a", 10, 20)];
    expect(nearestPassage(passages, 3)?.id).toBe("one");
    expect(nearestPassage(passages, 8)?.id).toBe("two");
    expect(nearestPassage(passages, 0)?.id).toBe("one");
    expect(nearestPassage(passages, 100)?.id).toBe("two");
    expect(nearestPassage([], 3)).toBeUndefined();
  });
});
