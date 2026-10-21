import { invoke } from "@tauri-apps/api/core";
import type { NewTimerLogInput } from "../types";

export function logTimerSession(input: NewTimerLogInput) {
  return invoke("log_timer_session", { input });
}

/** Total logged seconds for a workspace since its last reset. */
export function getWorkspaceTime(workspaceId: string): Promise<number> {
  return invoke("get_workspace_time", { workspaceId });
}

/**
 * Every workspace's total in one call, keyed by workspace id, each since its
 * own reset marker. Workspaces that have never been timed come back as 0 rather
 * than being absent.
 */
export function getAllWorkspaceTimes(): Promise<Record<string, number>> {
  return invoke("get_all_workspace_times");
}

/** Zeroes the accumulated total without deleting any session history. */
export function resetWorkspaceTime(workspaceId: string): Promise<void> {
  return invoke("reset_workspace_time", { workspaceId });
}
