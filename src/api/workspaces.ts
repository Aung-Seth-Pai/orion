import { invoke } from "@tauri-apps/api/core";
import type { NewWorkspaceInput, Workspace, WorkspacePatch } from "../types";

export function getWorkspaces(): Promise<Workspace[]> {
  return invoke("get_workspaces");
}

export function createWorkspace(input: NewWorkspaceInput): Promise<Workspace> {
  return invoke("create_workspace", { input });
}

export function updateWorkspace(id: string, patch: WorkspacePatch): Promise<Workspace> {
  return invoke("update_workspace", { id, patch });
}

export function deleteWorkspace(id: string): Promise<boolean> {
  return invoke("delete_workspace", { id });
}
