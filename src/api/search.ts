import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types";

export function searchAll(query: string): Promise<SearchResult[]> {
  return invoke("search_all", { query });
}
