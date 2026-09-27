export interface Workspace {
  id: string;
  name: string;
  color: string | null;
  icon: string | null;
  sortOrder: number;
  createdAt: string;
  /** Unix seconds of the last "Total time" reset; 0 means never reset. */
  timerResetAt: number;
}

export interface NewWorkspaceInput {
  name: string;
  color?: string | null;
  icon?: string | null;
}

export interface WorkspacePatch {
  name?: string;
  color?: string | null;
  icon?: string | null;
  sortOrder?: number;
}

export type ResourceType = "link" | "folder";

export type PreferredApp = "default" | "chrome" | "edge" | "firefox";

export interface Resource {
  id: string;
  workspaceId: string;
  type: ResourceType;
  title: string;
  targetPath: string;
  preferredApp: PreferredApp;
  profileName: string | null;
  sortOrder: number;
  createdAt: string;
}

export interface NewResourceInput {
  workspaceId: string;
  type: ResourceType;
  title: string;
  targetPath: string;
  preferredApp?: PreferredApp;
  profileName?: string | null;
}

export interface ResourcePatch {
  title?: string;
  targetPath?: string;
  type?: ResourceType;
  preferredApp?: PreferredApp;
  profileName?: string | null;
}

export type ScriptType = "powershell" | "cmd" | "python" | "node" | "wsl_bash";

export interface AutomationScript {
  id: string;
  workspaceId: string | null;
  title: string;
  scriptType: ScriptType;
  scriptContent: string;
  sortOrder: number;
  createdAt: string;
}

export interface NewScriptInput {
  workspaceId: string | null;
  title: string;
  scriptType: ScriptType;
  scriptContent: string;
}

export interface ScriptPatch {
  title?: string;
  scriptType?: ScriptType;
  scriptContent?: string;
}

export type SessionType = "stopwatch" | "pomodoro";

export interface TimerLog {
  id: string;
  workspaceId: string;
  durationSeconds: number;
  sessionType: SessionType;
  startedAt: string;
  endedAt: string;
}

export interface NewTimerLogInput {
  workspaceId: string;
  durationSeconds: number;
  sessionType: SessionType;
}

export type SearchResultAction =
  | "open_workspace"
  | "launch_resource"
  | "execute_script"
  /** A non-actionable row, used by the "enable semantic search" notice. */
  | "none";

export interface SearchResult {
  itemType: "workspace" | "link" | "folder" | "script" | "notice";
  id: string;
  title: string;
  subtitle: string;
  action: SearchResultAction;
}

/**
 * An existing resource offered as a possible duplicate while adding a new one.
 * Mirrors `models::ResourceSuggestion`.
 */
export interface ResourceSuggestion {
  id: string;
  type: ResourceType;
  title: string;
  targetPath: string;
  workspaceId: string;
  workspaceName: string;
  /** "exact" reads as "you already have this"; the others as a resemblance. */
  matchKind: "exact" | "keyword" | "semantic";
}

/** Contents of the always-on-top timer alert window. Mirrors `alert::AlertPayload`. */
export interface AlertPayload {
  title: string;
  body: string;
  /**
   * Distinguishes consecutive alerts with identical text so the window can
   * restart its auto-dismiss countdown instead of treating the second as a
   * repeat render of the first.
   */
  nonce: number;
}

/** State of the local semantic search feature, shown in Settings. */
export interface AiStatus {
  /** Whether the embedding model is present on disk and ready to use. */
  downloaded: boolean;
  /** How many items currently have a vector. */
  indexedCount: number;
  /** How many items exist in total, so a stale index is visible as a mismatch. */
  indexableCount: number;
}

export interface Task {
  id: string;
  workspaceId: string;
  title: string;
  isCompleted: boolean;
  createdAt: string;
}

export interface NewTaskInput {
  workspaceId: string;
  title: string;
}

export interface EnvVar {
  id: string;
  workspaceId: string;
  envKey: string;
  envValue: string;
}

export type TimerPhase = "idle" | "running" | "done";

export interface TimerData {
  mode: SessionType;
  phase: TimerPhase;
  elapsed: number;
  startedAt: number | null;
}

export const IDLE_TIMER: TimerData = {
  mode: "stopwatch",
  phase: "idle",
  elapsed: 0,
  startedAt: null,
};

export interface TimerController {
  states: Record<string, TimerData>;
  pomodoroSeconds: number;
  /**
   * Bumped once a finished session has been written to SQLite. Views showing
   * accumulated totals watch this instead of the timer phase, which changes
   * before the insert completes.
   */
  logVersion: number;
  start(workspaceId: string, mode: SessionType): void;
  stop(workspaceId: string): void;
  reset(workspaceId: string): void;
}
