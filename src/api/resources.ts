import { invoke } from "@tauri-apps/api/core";
import type { NewResourceInput, Resource, ResourcePatch } from "../types";

export function getResources(workspaceId: string): Promise<Resource[]> {
  return invoke("get_resources", { workspaceId });
}

export function createResource(input: NewResourceInput): Promise<Resource> {
  return invoke("create_resource", { input });
}

export function updateResource(id: string, patch: ResourcePatch): Promise<Resource> {
  return invoke("update_resource", { id, patch });
}

export function deleteResource(id: string): Promise<boolean> {
  return invoke("delete_resource", { id });
}

export function launchResource(id: string, actionOverride?: "ide" | "terminal"): Promise<string> {
  return invoke("launch_resource", { id, actionOverride: actionOverride ?? null });
}

/**
 * Persists a drag-and-drop reorder. `ids` must be every resource in the
 * workspace, in order — the backend refuses a partial list rather than leave
 * the unlisted rows sharing sort_order values.
 */
export function reorderResources(workspaceId: string, ids: string[]): Promise<void> {
  return invoke("reorder_resources", { workspaceId, ids });
}
