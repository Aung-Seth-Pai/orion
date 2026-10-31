import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";

export function getSettings(): Promise<Record<string, string>> {
  return invoke("get_settings");
}

export function updateSetting(key: string, value: string): Promise<void> {
  return invoke("update_setting", { key, value });
}

/**
 * A password encrypts the backup; null leaves it as readable JSON.
 *
 * Deriving the key is deliberately slow -- around half a second -- so this
 * takes noticeably longer with a password than without one.
 */
export function exportAllData(password: string | null): Promise<string> {
  return invoke("export_all_data", { password });
}

/**
 * Runs export_all_data and writes the result to a location chosen in the
 * native save dialog. Returns the chosen file path, or null if the user
 * cancelled the dialog.
 *
 * The extension stays .json either way. An encrypted backup is still a JSON
 * file -- an envelope whose payload is base64 -- so the save dialog, the file
 * picker on the way back in, and every sync tool keep behaving as they do now.
 */
export async function exportBackupToFile(
  password: string | null
): Promise<string | null> {
  const filePath = await save({
    defaultPath: "orion_backup.json",
    filters: [{ name: "JSON backup", extensions: ["json"] }],
  });
  if (!filePath) return null;

  // Only after a path is chosen, so a cancelled dialog does not spend half a
  // second deriving a key for a file nobody will write.
  const json = await exportAllData(password);
  await writeTextFile(filePath, json);
  return filePath;
}

/** What can be told about a backup file without being able to open it. */
export interface BackupInfo {
  encrypted: boolean;
}

/**
 * Asks the backend what kind of file this is, so the import dialog knows
 * whether to ask for a password.
 *
 * The question is answered in Rust rather than by looking at the JSON here,
 * so there is one definition of the envelope format instead of two that can
 * drift apart.
 */
export function inspectBackup(text: string): Promise<BackupInfo> {
  return invoke("inspect_backup", { text });
}

/** What an import brought in, so the UI can report it rather than just "done". */
export interface ImportSummary {
  workspaces: number;
  resources: number;
  scripts: number;
  timerLogs: number;
  tasks: number;
  envVars: number;
  /** Titles of imported scripts that cannot run on this platform (capped). */
  unrunnableScripts: string[];
  /** How many there were in total, which may exceed the titles listed. */
  unrunnableCount: number;
  /** The script types involved, already labelled for display. */
  unrunnableTypes: string[];
}

/** `password` is required for an encrypted backup and ignored for a plain one. */
export function importData(
  jsonPayload: string,
  password: string | null
): Promise<ImportSummary> {
  return invoke("import_data", { jsonPayload, password });
}


/**
 * Changes the global Spotlight shortcut. Rejects when the accelerator cannot be
 * registered — usually because another application already owns it — in which
 * case the previous shortcut is left in place and still working.
 */
export function setSpotlightShortcut(accelerator: string): Promise<void> {
  return invoke("set_spotlight_shortcut", { accelerator });
}
