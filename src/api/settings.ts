import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";

export function getSettings(): Promise<Record<string, string>> {
  return invoke("get_settings");
}

export function updateSetting(key: string, value: string): Promise<void> {
  return invoke("update_setting", { key, value });
}

export function exportAllData(): Promise<string> {
  return invoke("export_all_data");
}

/**
 * Runs export_all_data and writes the JSON to a location chosen in the
 * native save dialog. Returns the chosen file path, or null if the user
 * cancelled the dialog.
 */
export async function exportBackupToFile(): Promise<string | null> {
  const filePath = await save({
    defaultPath: "orion_backup.json",
    filters: [{ name: "JSON backup", extensions: ["json"] }],
  });
  if (!filePath) return null;

  const json = await invoke<string>("export_all_data");
  await writeTextFile(filePath, json);
  return filePath;
}

export function importData(jsonPayload: string): Promise<void> {
  return invoke("import_data", { jsonPayload });
}


/**
 * Changes the global Spotlight shortcut. Rejects when the accelerator cannot be
 * registered — usually because another application already owns it — in which
 * case the previous shortcut is left in place and still working.
 */
export function setSpotlightShortcut(accelerator: string): Promise<void> {
  return invoke("set_spotlight_shortcut", { accelerator });
}
