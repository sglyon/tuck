export type MeetingStatus =
  | "draft"
  | "recording"
  | "paused"
  | "processing"
  | "complete"
  | "interrupted";
export type NotionStatus =
  | "idle"
  | "syncing"
  | "synced"
  | "error"
  | "uncertain";
export interface Segment {
  id: string;
  meetingId: string;
  start: number;
  end: number;
  text: string;
  originalText?: string;
  edited?: boolean;
  correctedAt?: string;
  speakerId: string;
  source: "microphone" | "system" | "import";
}
export interface Speaker {
  id: string;
  name: string;
  color: number;
}
export interface ActionItem {
  id: string;
  title: string;
  owner: string;
  dueDate: string;
  done: boolean;
  createdAt: string;
}
export interface CoachSuggestion {
  id: string;
  kind: "question" | "follow_up" | "reminder" | "insight";
  agentId: string;
  agentName: string;
  quote: string;
  text: string;
  segmentId: string;
  start: number;
  createdAt: string;
  status: "open" | "dismissed" | "saved";
  taskId?: string | null;
}
export interface LiveCoachState {
  enabled: boolean;
  selectedAgentIds: string[];
  status: "off" | "waiting" | "analyzing" | "paused" | "unavailable" | "stopped";
  message?: string | null;
  suggestions: CoachSuggestion[];
}
export interface MeetingAgent {
  id: string;
  name: string;
  instructions: string;
  pace: "quiet" | "active";
}
export interface CalendarEvent {
  id: string;
  source: string;
  title: string;
  start: string;
  end: string;
  joinUrl: string;
  attendees: string[];
  allDay?: boolean;
}
export interface Template {
  id: string;
  name: string;
  instructions: string;
}
export interface Recipe {
  id: string;
  name: string;
  prompt: string;
}
export interface SearchHit {
  meetingId: string;
  meetingTitle: string;
  segmentId?: string;
  start?: number;
  text: string;
  source: string;
  url?: string;
}
export interface Revision {
  id: string;
  target: "notes" | "summary";
  text: string;
  createdAt: string;
}
export interface Proposal {
  text: string;
  baseText: string;
  target: "notes" | "summary";
}
export interface Message {
  id: string;
  citations?: SearchHit[];
  role: "user" | "assistant";
  content: string;
  createdAt: string;
}
export interface Bookmark {
  id: string;
  time: number;
  label: string;
}
export interface Meeting {
  kind?: "meeting" | "dictation";
  id: string;
  title: string;
  createdAt: string;
  duration: number;
  status: MeetingStatus;
  notes: string;
  summary: string;
  template: string;
  starred: boolean;
  isExample: boolean;
  notionUrl: string | null;
  notionStatus: NotionStatus;
  segments: Segment[];
  speakers: Speaker[];
  messages: Message[];
  bookmarks: Bookmark[];
  folder?: string;
  tags?: string[];
  people?: string[];
  tasks?: ActionItem[];
  calendarEvent?: CalendarEvent | null;
  recoveredFrom?: string | null;
}
export interface Settings {
  meetingAgents?: MeetingAgent[];
  provider: "local" | "chatgpt" | "openai";
  model: string;
  localUrl: string;
  hostedConsent: boolean;
  webSearch: boolean;
  autoSummary: boolean;
  notionAutoSync: boolean;
  notionContent: { recap: boolean; notes: boolean; tasks: boolean };
  notionTarget: string;
  notionPropertyMapping: Record<string, string>;
  notionSetup?: {
    parent: string;
    status: "creating" | "uncertain" | "ready";
    destination?: { id: string; title: string; url: string };
    message?: string;
  } | null;
  threads: number;
  keepAudio: boolean;
  hasOpenAIKey: boolean;
  hasNotionToken: boolean;
  customTemplates: Template[];
  recipes: Recipe[];
  microphoneDeviceId: string;
  systemDeviceId: string;
  meetingReminders: boolean;
}
export interface EngineStatus {
  engineVersion: string;
  platform: string;
  modelInstalled: boolean;
  modelDownloading: boolean;
  modelProgress: number;
  speakerModelInstalled: boolean;
  activeMeetingId: string | null;
  recordingState: string;
  captureAvailable: boolean;
  error?: string;
}
export interface GlobalDictationStatus {
  shortcut: string;
  shortcutAvailable: boolean;
  state: "idle" | "starting" | "listening" | "processing" | "copied" | "paste_sent" | "error";
  message?: string | null;
  meetingId?: string | null;
  pasteAvailable?: boolean;
  pastePermission?: "granted" | "denied" | "unavailable" | "unknown";
}
export interface Account {
  id: string;
  email: string;
  label: string;
  connected: boolean | null;
  sharing: boolean | null;
  active: boolean;
}
export interface ModelOption {
  id: string;
  name: string;
}
export interface Bootstrap {
  meetings: Meeting[];
  settings: Settings;
  status: EngineStatus;
  accounts: Account[];
}
export interface EngineEvent {
  event: string;
  payload: Record<string, unknown>;
}
export type MeetingChanges = Partial<
  Pick<
    Meeting,
    | "title"
    | "notes"
    | "summary"
    | "template"
    | "starred"
    | "folder"
    | "tags"
    | "people"
    | "calendarEvent"
  >
>;
export type ExportFormat = "markdown" | "json" | "srt";

export interface TranscriptDraft {
  meetingId: string;
  segmentId: string;
  baseText: string;
  text: string;
}
export interface RetainedAudioFile {
  id: string;
  name: string;
  bytes: number;
  duration?: number;
  repairable?: boolean;
}

export type MeetingHeader = Pick<
  Meeting,
  | "id"
  | "title"
  | "kind"
  | "createdAt"
  | "duration"
  | "status"
  | "template"
  | "starred"
  | "isExample"
  | "notionUrl"
  | "notionStatus"
  | "folder"
  | "tags"
  | "people"
  | "calendarEvent"
  | "recoveredFrom"
> & {
  segmentCount: number;
  speakerCount: number;
  messageCount: number;
  bookmarkCount: number;
  taskCount: number;
  openTaskCount: number;
  hasNotes: boolean;
  hasSummary: boolean;
};
export interface MeetingPage {
  meetings: MeetingHeader[];
  nextCursor: string | null;
  total: number;
}
export interface LibraryOverview {
  meetingCount: number;
  starredCount: number;
  openTaskCount: number;
  folders: string[];
  tags: string[];
  people: string[];
  facetsTruncated: { folders: boolean; tags: boolean; people: boolean };
  activeMeetingId: string | null;
}
export interface LibraryBootstrap {
  meetingPage: MeetingPage;
  library: LibraryOverview;
  settings: Settings;
  status: EngineStatus;
  accounts: Account[];
}
export type LibraryTask = ActionItem & {
  meetingId: string;
  meetingTitle: string;
};
export interface TaskPage {
  tasks: LibraryTask[];
  nextCursor: string | null;
  total: number;
}
