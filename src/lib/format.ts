import type { Meeting, ExportFormat } from "./types";
export function clock(seconds: number): string {
  const n = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return n >= 3600
    ? `${Math.floor(n / 3600)}:${String(Math.floor(n / 60) % 60).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}`
    : `${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}`;
}
export function duration(seconds: number): string {
  return seconds < 60
    ? `${Math.floor(seconds)} sec`
    : `${Math.round(seconds / 60)} min`;
}
export function meetingDate(date: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(date));
}
export function fullDate(date: string): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(date));
}
export function relativeGroup(date: string): string {
  const d = new Date(date),
    now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const diff = Math.floor(
    (start.getTime() -
      new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) /
      86400000,
  );
  return diff === 0
    ? "Today"
    : diff === 1
      ? "Yesterday"
      : diff < 7
        ? "This week"
        : "Earlier";
}
export function initials(name: string): string {
  return (
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0])
      .join("")
      .toUpperCase() || "?"
  );
}
export function exportContent(meeting: Meeting, format: ExportFormat): string {
  if (format === "json") return JSON.stringify(meeting, null, 2);
  if (format === "srt")
    return meeting.segments
      .map((s, i) => {
        const stamp = (v: number) => {
          const ms = Math.round(v * 1000);
          return `${String(Math.floor(ms / 3600000)).padStart(2, "0")}:${String(Math.floor(ms / 60000) % 60).padStart(2, "0")}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")},${String(ms % 1000).padStart(3, "0")}`;
        };
        return `${i + 1}\n${stamp(s.start)} --> ${stamp(s.end)}\n${meeting.speakers.find((p) => p.id === s.speakerId)?.name ?? "Speaker"}: ${s.edited ? "[User-corrected] " : ""}${s.text}\n`;
      })
      .join("\n");
  return `# ${meeting.title}\n\n${fullDate(meeting.createdAt)} · ${duration(meeting.duration)}${meeting.isExample ? " · Illustrative example" : ""}\n\n## Summary\n\n${meeting.summary || "No summary yet."}\n\n## My notes\n\n${meeting.notes || "No notes."}\n\n## Transcript\n\n${meeting.segments.map((s) => `[${clock(s.start)}] **${meeting.speakers.find((p) => p.id === s.speakerId)?.name ?? "Speaker"}:** ${s.edited ? "[User-corrected] " : ""}${s.text}`).join("\n\n")}`;
}
