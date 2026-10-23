import { describe, it, expect } from "vitest";
import { clock, duration, exportContent, initials } from "./format";
import { exampleMeeting } from "./example";
describe("time and speaker display", () => {
  it("formats transcript positions and hour boundaries without losing seconds", () => {
    expect(clock(65.8)).toBe("01:05");
    expect(clock(3601)).toBe("1:00:01");
    expect(clock(-1)).toBe("00:00");
    expect(clock(Number.NaN)).toBe("00:00");
  });
  it("shows names without inferring an identity", () => {
    expect(initials("Maya Chen")).toBe("MC");
    expect(initials("Speaker 1")).toBe("S1");
    expect(initials("")).toBe("?");
  });
});
describe("portable meeting exports", () => {
  it("keeps speaker names and exact subtitle times", () => {
    const m = exampleMeeting();
    m.segments = m.segments.slice(0, 1);
    m.segments[0].start = 1.125;
    m.segments[0].end = 5.5;
    expect(exportContent(m, "srt")).toContain(
      "00:00:01,125 --> 00:00:05,500\nYou: Thanks",
    );
  });
  it("retains notes and source transcript separately from AI summary", () => {
    const m = exampleMeeting();
    const md = exportContent(m, "markdown");
    expect(md).toContain("## Summary");
    expect(md).toContain("## My notes");
    expect(md).toContain("## Transcript");
    expect(md).toContain("Illustrative example");
    expect(JSON.parse(exportContent(m, "json")).id).toBe(m.id);
  });
  it("labels corrected exports while JSON retains original speech evidence", () => {
    const meeting = exampleMeeting();
    meeting.segments = meeting.segments.slice(0, 1);
    const original = meeting.segments[0].text;
    Object.assign(meeting.segments[0], {
      originalText: original,
      text: "Corrected by the user.",
      edited: true,
    });
    expect(exportContent(meeting, "markdown")).toContain(
      "[User-corrected] Corrected by the user.",
    );
    expect(exportContent(meeting, "srt")).toContain(
      "[User-corrected] Corrected by the user.",
    );
    expect(
      JSON.parse(exportContent(meeting, "json")).segments[0].originalText,
    ).toBe(original);
  });
  it("uses independent example objects instead of mutable shared meetings", () => {
    const a = exampleMeeting(),
      b = exampleMeeting();
    expect(a.id).not.toBe(b.id);
    a.speakers[0].name = "Changed";
    expect(b.speakers[0].name).toBe("You");
    expect(a.segments.every((s) => s.meetingId === a.id)).toBe(true);
  });
});
