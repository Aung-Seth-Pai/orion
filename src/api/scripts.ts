import { invoke } from "@tauri-apps/api/core";
import type {
  AutomationScript,
  NewScriptInput,
  ScriptPatch,
} from "../types";

export function getScripts(workspaceId: string | null): Promise<AutomationScript[]> {
  return invoke("get_scripts", { workspaceId });
}

export function createScript(input: NewScriptInput): Promise<AutomationScript> {
  return invoke("create_script", { input });
}

export function updateScript(id: string, patch: ScriptPatch): Promise<AutomationScript> {
  return invoke("update_script", { id, patch });
}

export function deleteScript(id: string): Promise<boolean> {
  return invoke("delete_script", { id });
}

export function executeScript(id: string): Promise<string> {
  return invoke("execute_script", { id });
}
