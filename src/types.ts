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
  | "execute_script";

export interface SearchResult {
  itemType: "workspace" | "link" | "folder" | "script";
  id: string;
  title: string;
  subtitle: string;
  action: SearchResultAction;
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
