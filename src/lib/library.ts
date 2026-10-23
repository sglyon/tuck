import type { LibraryOverview, Meeting, MeetingHeader } from "./types";
export const emptyLibrary: LibraryOverview = {
  meetingCount: 0,
  starredCount: 0,
  openTaskCount: 0,
  folders: [],
  tags: [],
  people: [],
  facetsTruncated: { folders: false, tags: false, people: false },
  activeMeetingId: null,
};
export function meetingHeader(m: Meeting): MeetingHeader {
  const {
    id,
    title,
    kind,
    createdAt,
    duration,
    status,
    template,
    starred,
    isExample,
    notionUrl,
    notionStatus,
    folder = "",
    tags = [],
    people = [],
    calendarEvent = null,
    recoveredFrom = null,
  } = m;
  return {
    id,
    title,
    kind,
    createdAt,
    duration,
    status,
    template,
    starred,
    isExample,
    notionUrl,
    notionStatus,
    folder,
    tags,
    people,
    calendarEvent,
    recoveredFrom,
    segmentCount: m.segments.length,
    speakerCount: m.speakers.length,
    messageCount: m.messages.length,
    bookmarkCount: m.bookmarks.length,
    taskCount: (m.tasks || []).length,
    openTaskCount: (m.tasks || []).filter((t) => !t.done).length,
    hasNotes: !!m.notes,
    hasSummary: !!m.summary,
  };
}
export function libraryScope(scope: {
  folder: string;
  tag: string;
  person: string;
}) {
  return {
    folder: scope.folder || undefined,
    tags: scope.tag ? [scope.tag] : undefined,
    people: scope.person ? [scope.person] : undefined,
  };
}
export function cursorStale(error: unknown) {
  return (
    !!error &&
    typeof error === "object" &&
    "code" in error &&
    error.code === "cursor_stale"
  );
}
export function pruneDetails(
  meetings: Meeting[],
  protectedIds: Set<string>,
  lastUse: Map<string, number>,
  idleLimit = 5,
) {
  const idle = meetings
    .filter(
      (m) =>
        !protectedIds.has(m.id) &&
        !["recording", "paused", "processing"].includes(m.status),
    )
    .sort((a, b) => (lastUse.get(b.id) || 0) - (lastUse.get(a.id) || 0));
  const keep = new Set(idle.slice(0, idleLimit).map((m) => m.id));
  const next = meetings.filter(
    (m) =>
      protectedIds.has(m.id) ||
      ["recording", "paused", "processing"].includes(m.status) ||
      keep.has(m.id),
  );
  return next.length === meetings.length ? meetings : next;
}
