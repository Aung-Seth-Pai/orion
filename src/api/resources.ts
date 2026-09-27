import { invoke } from "@tauri-apps/api/core";
import type {
  NewResourceInput,
  Resource,
  ResourcePatch,
  ResourceSuggestion,
  ResourceType,
} from "../types";

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

/**
 * Existing resources resembling `term`, across every workspace — the question is
 * "have I saved this already", and the answer matters most when it is elsewhere.
 * Keyword matching always works; semantic matching contributes only when the
 * embedding index exists, and never triggers a model download.
 */
export function suggestResources(term: string): Promise<ResourceSuggestion[]> {
  return invoke("suggest_resources", { term });
}

/**
 * The resource already pointing at `target`, if any. Separate from
 * suggestResources because an exact duplicate is a different statement and is
 * worded differently. The target is normalised first, so a bare path matches the
 * file:// URL it would be stored as.
 */
export function findResourceByTarget(
  resourceType: ResourceType,
  target: string
): Promise<ResourceSuggestion | null> {
  return invoke("find_resource_by_target", { resourceType, target });
}
