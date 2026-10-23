import { expect, it } from "vitest";
import { exampleMeeting } from "./example";
import { meetingHeader, pruneDetails } from "./library";
it("keeps selected, dirty and capture owners while evicting least recently used idle detail bodies", () => {
  const meetings = Array.from({ length: 12 }, (_, i) => ({
    ...exampleMeeting(),
    id: String(i),
    status: i === 2 ? ("recording" as const) : ("complete" as const),
  }));
  const next = pruneDetails(
    meetings,
    new Set(["0", "1"]),
    new Map(meetings.map((m, i) => [m.id, i])),
  );
  expect(next.map((m) => m.id)).toEqual([
    "0",
    "1",
    "2",
    "7",
    "8",
    "9",
    "10",
    "11",
  ]);
  expect(next.find((m) => m.id === "0")).toBe(meetings[0]);
  const header = meetingHeader(meetings[0]);
  expect(header.segmentCount).toBe(meetings[0].segments.length);
  expect(header).not.toHaveProperty("segments");
  expect(header).not.toHaveProperty("notes");
  expect(header).not.toHaveProperty("tasks");
});
