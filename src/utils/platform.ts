/**
 * What the host platform can do, fetched once from Rust.
 *
 * Nothing here is derived in TypeScript. The backend owns these answers because
 * it owns the launcher and the interpreters: if the script dropdown offered a
 * type the runner rejects, or the browser picker offered one the launcher
 * cannot resolve, the mismatch would only show up when somebody pressed the
 * button. One fetch at startup, then a synchronous accessor — the same shape as
 * `theme.ts`, for the same reason: components read it during render.
 */

import { invoke } from "@tauri-apps/api/core";

export interface PlatformInfo {
  os: "windows" | "macos";
  /** Script types this machine can execute, in the order to offer them. */
  runnableScriptTypes: string[];
  /** Every type Orion can store, so an inert one can still be labelled. */
  allScriptTypes: string[];
  /** `preferredApp` values with a launcher here. */
  browsers: string[];
  terminalLabel: string;
  fileManagerLabel: string;
  pathSeparator: string;
  /** The accelerator a fresh install binds; it differs per platform. */
  defaultShortcut: string;
}

/**
 * Used until the real answer arrives, and if the call ever fails.
 *
 * Windows is the right fallback rather than a neutral guess: it is what the
 * overwhelming majority of installs are, and the values are only ever used for
 * labels and for which options to offer. A wrong guess here shows "Explorer"
 * for a few milliseconds; it cannot cause a script to run or fail to run,
 * because that decision is made again in Rust.
 */
const FALLBACK: PlatformInfo = {
  os: "windows",
  runnableScriptTypes: ["powershell", "cmd", "wsl_bash", "python", "node"],
  allScriptTypes: ["powershell", "cmd", "wsl_bash", "python", "node", "zsh", "bash"],
  browsers: ["default", "chrome", "edge", "firefox"],
  terminalLabel: "Windows Terminal",
  fileManagerLabel: "Explorer",
  pathSeparator: "\\",
  defaultShortcut: "CmdOrCtrl+Shift+O",
};

let current: PlatformInfo = FALLBACK;

/**
 * The host platform. Safe to call during render, in an effect, or in a handler.
 *
 * NOT safe to call at module scope. Module bodies run while `main.tsx` is still
 * awaiting `loadPlatform()`, so `const TYPES = platform().runnableScriptTypes`
 * at the top of a component file captures the fallback on every platform and
 * never updates. Read it inside the function that needs it.
 */
export function platform(): PlatformInfo {
  return current;
}

export function isMac(): boolean {
  return current.os === "macos";
}

/**
 * Fetches the real values. Called once before the first render; a failure
 * leaves the fallback in place rather than blocking the app from starting.
 */
export async function loadPlatform(): Promise<PlatformInfo> {
  try {
    const info = await invoke<PlatformInfo>("get_platform_info");
    // A newer backend could add a field; only replace what we recognise, and
    // only if it looks like the shape we expect.
    if (info && typeof info.os === "string" && info.runnableScriptTypes?.length) {
      current = info;
    }
  } catch {
    // Keep the fallback. The real decisions are enforced in Rust anyway.
  }
  return current;
}

/** True when this machine can run `scriptType`. */
export function canRunScript(scriptType: string): boolean {
  return current.runnableScriptTypes.includes(scriptType);
}

/** How to write a script type for a human, on any platform. */
export function scriptLabel(scriptType: string): string {
  switch (scriptType) {
    case "powershell":
      return "PowerShell";
    case "cmd":
      return "CMD";
    case "wsl_bash":
      return "WSL Bash";
    case "python":
      return "Python";
    case "node":
      return "Node.js";
    case "zsh":
      return "zsh";
    case "bash":
      return "bash";
    default:
      return scriptType;
  }
}

/**
 * Why a stored script cannot run here, or `null` when it can.
 *
 * Phrased so that it reads as a fact about platforms rather than as a fault in
 * the user's script — it has not been altered and will not be deleted, and it
 * still works on the machine it was written for.
 */
export function unrunnableReason(scriptType: string): string | null {
  if (canRunScript(scriptType)) return null;
  const home =
    scriptType === "powershell" || scriptType === "cmd" || scriptType === "wsl_bash"
      ? "Windows"
      : scriptType === "zsh" || scriptType === "bash"
        ? "macOS"
        : null;
  if (!home) {
    return `Orion does not know how to run '${scriptType}' scripts.`;
  }
  return `${scriptLabel(scriptType)} scripts only run on ${home}. This one is kept exactly as written, so it still works when you open this setup on ${home}.`;
}

/**
 * Renders an accelerator the way this platform writes it.
 *
 * Shortcuts are stored as `CmdOrCtrl+Shift+O` so the same stored value binds
 * correctly on either platform; only the display differs. Mac conventions are
 * glyphs with no separators, which is what every other Mac app shows.
 */
export function displayAccelerator(accelerator: string, separator = "+"): string {
  if (!isMac()) {
    return accelerator
      .split("+")
      .map((part) => (part === "CmdOrCtrl" ? "Ctrl" : part))
      .join(separator);
  }

  // Mac conventions put no separator between glyphs, whatever the caller
  // would have used between words.
  const glyphs: Record<string, string> = {
    CmdOrCtrl: "⌘",
    Cmd: "⌘",
    Command: "⌘",
    Control: "⌃",
    Ctrl: "⌃",
    Alt: "⌥",
    Option: "⌥",
    Shift: "⇧",
  };
  // And they are always written in this order, however the accelerator spelled
  // them: a stored `CmdOrCtrl+Alt+O` reads as ⌥⌘O on a Mac, never ⌘⌥O.
  const order = ["⌃", "⌥", "⇧", "⌘"];
  const parts = accelerator.split("+").map((part) => glyphs[part] ?? part);
  const modifiers = parts
    .filter((p) => order.includes(p))
    .sort((a, b) => order.indexOf(a) - order.indexOf(b));
  const keys = parts.filter((p) => !order.includes(p));
  return [...modifiers, ...keys].join("");
}
