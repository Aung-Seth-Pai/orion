import { beforeEach, describe, expect, it, vi } from "vitest";

// vi.hoisted, because vi.mock is lifted above every other statement in the
// file — a plain `const invoke = vi.fn()` here is not yet initialised when the
// factory runs.
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  canRunScript,
  displayAccelerator,
  isMac,
  loadPlatform,
  platform,
  scriptLabel,
  unrunnableReason,
} from "./platform";

const MAC = {
  os: "macos" as const,
  runnableScriptTypes: ["zsh", "bash", "python", "node"],
  allScriptTypes: ["powershell", "cmd", "wsl_bash", "python", "node", "zsh", "bash"],
  browsers: ["default", "safari", "chrome", "edge", "firefox"],
  terminalLabel: "Terminal",
  fileManagerLabel: "Finder",
  pathSeparator: "/",
  defaultShortcut: "CmdOrCtrl+Alt+O",
};

const WINDOWS = {
  os: "windows" as const,
  runnableScriptTypes: ["powershell", "cmd", "wsl_bash", "python", "node"],
  allScriptTypes: MAC.allScriptTypes,
  browsers: ["default", "chrome", "edge", "firefox"],
  terminalLabel: "Windows Terminal",
  fileManagerLabel: "Explorer",
  pathSeparator: "\\",
  defaultShortcut: "CmdOrCtrl+Shift+O",
};

/** Puts the module into a known platform, as `main.tsx` does at startup. */
async function become(info: unknown) {
  invoke.mockResolvedValueOnce(info);
  await loadPlatform();
}

beforeEach(() => {
  invoke.mockReset();
});

describe("loadPlatform", () => {
  it("adopts what the backend reports", async () => {
    await become(MAC);
    expect(platform().os).toBe("macos");
    expect(isMac()).toBe(true);
    expect(platform().fileManagerLabel).toBe("Finder");
  });

  it("keeps the fallback when the call fails", async () => {
    await become(WINDOWS);
    invoke.mockRejectedValueOnce(new Error("ipc unavailable"));

    await expect(loadPlatform()).resolves.toBeDefined();
    // The previous answer stands rather than the app losing its labels. Every
    // decision that matters is enforced again in Rust, so a stale value here
    // cannot run a script that should not run.
    expect(platform().os).toBe("windows");
  });

  it("ignores a malformed response", async () => {
    await become(WINDOWS);
    for (const junk of [null, {}, { os: "macos" }, { os: "macos", runnableScriptTypes: [] }]) {
      invoke.mockResolvedValueOnce(junk);
      await loadPlatform();
      expect(platform().os).toBe("windows");
    }
  });
});

describe("canRunScript", () => {
  it("follows the platform, not the stored type list", async () => {
    await become(MAC);
    expect(canRunScript("zsh")).toBe(true);
    expect(canRunScript("python")).toBe(true);
    expect(canRunScript("powershell")).toBe(false);

    await become(WINDOWS);
    expect(canRunScript("powershell")).toBe(true);
    expect(canRunScript("zsh")).toBe(false);
    // Portable either way — the whole reason the crossing is worth making.
    expect(canRunScript("python")).toBe(true);
    expect(canRunScript("node")).toBe(true);
  });
});

describe("unrunnableReason", () => {
  it("says nothing about a script that runs here", async () => {
    await become(MAC);
    expect(unrunnableReason("bash")).toBeNull();
  });

  it("names the platform the script does work on", async () => {
    await become(MAC);
    const reason = unrunnableReason("powershell");
    expect(reason).toContain("Windows");
    // It must not read as though the script were damaged or will be lost.
    expect(reason).toContain("kept");
  });

  it("explains an unknown type from a newer backup", async () => {
    await become(WINDOWS);
    expect(unrunnableReason("fish")).toContain("fish");
  });
});

describe("scriptLabel", () => {
  it("matches the vocabulary the backend reports in an import summary", () => {
    // These two strings are duplicated in Rust's platform::script_label. If
    // they drift, an import summary and a script badge name the same type
    // differently in the same window.
    expect(scriptLabel("powershell")).toBe("PowerShell");
    expect(scriptLabel("cmd")).toBe("CMD");
    expect(scriptLabel("wsl_bash")).toBe("WSL Bash");
    expect(scriptLabel("node")).toBe("Node.js");
    expect(scriptLabel("zsh")).toBe("zsh");
  });

  it("falls back to the raw id rather than blank", () => {
    expect(scriptLabel("fish")).toBe("fish");
  });
});

describe("displayAccelerator", () => {
  it("spells out modifiers on Windows", async () => {
    await become(WINDOWS);
    expect(displayAccelerator("CmdOrCtrl+Shift+O")).toBe("Ctrl+Shift+O");
    expect(displayAccelerator("CmdOrCtrl+Shift+O", " + ")).toBe("Ctrl + Shift + O");
  });

  it("uses glyphs with no separator on a Mac", async () => {
    await become(MAC);
    expect(displayAccelerator("CmdOrCtrl+Shift+O")).toBe("⇧⌘O");
    // The separator a caller would have used between words is dropped, because
    // no Mac app writes "⇧ + ⌘ + O".
    expect(displayAccelerator("CmdOrCtrl+Shift+O", " + ")).toBe("⇧⌘O");
    expect(displayAccelerator("Alt+Space")).toBe("⌥Space");
  });

  it("writes modifiers in the order a Mac writes them", async () => {
    await become(MAC);
    // Stored Cmd-first, displayed Option-first: Apple's order is always
    // Control, Option, Shift, Command, whatever the accelerator says.
    expect(displayAccelerator("CmdOrCtrl+Alt+O")).toBe("⌥⌘O");
    expect(displayAccelerator("CmdOrCtrl+Alt+Shift+K")).toBe("⌥⇧⌘K");
    expect(displayAccelerator("CmdOrCtrl+Control+Alt+Shift+P")).toBe("⌃⌥⇧⌘P");
  });

  it("carries the platform's own default", async () => {
    await become(MAC);
    expect(displayAccelerator(platform().defaultShortcut)).toBe("⌥⌘O");
    await become(WINDOWS);
    expect(displayAccelerator(platform().defaultShortcut)).toBe("Ctrl+Shift+O");
  });

  it("leaves a key it does not recognise alone", async () => {
    await become(MAC);
    expect(displayAccelerator("CmdOrCtrl+F13")).toBe("⌘F13");
  });
});
