import { invoke } from "@tauri-apps/api/core";
import type { EnvVar } from "../types";

export function getEnvVars(workspaceId: string): Promise<EnvVar[]> {
  return invoke("get_env_vars", { workspaceId });
}

export function setEnvVar(
  workspaceId: string,
  envKey: string,
  envValue: string
): Promise<EnvVar> {
  return invoke("set_env_var", { workspaceId, envKey, envValue });
}

export function deleteEnvVar(id: string): Promise<boolean> {
  return invoke("delete_env_var", { id });
}
