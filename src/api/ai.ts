import { invoke } from "@tauri-apps/api/core";
import type { AiStatus } from "../types";

/** Whether the embedding model is ready, and how much of the index is built. */
export function getAiStatus(): Promise<AiStatus> {
  return invoke("get_ai_status");
}

/**
 * Downloads the model if it is missing (~90 MB, once) and rebuilds the whole
 * vector index. This is the only call that ever fetches the model, so it must
 * only run from an explicit user action.
 */
export function reindexAll(): Promise<void> {
  return invoke("reindex_all");
}
