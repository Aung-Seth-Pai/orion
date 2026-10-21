import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  Download,
  FolderOpen,
  Loader2,
  RefreshCw,
  Sparkles,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";
import {
  exportBackupToFile,
  getSettings,
  importData,
  setSpotlightShortcut,
  updateSetting,
} from "../api/settings";
import type { ImportSummary } from "../api/settings";
import { getAiStatus, reindexAll, removeAiModel } from "../api/ai";
import { platform } from "../utils/platform";
import ShortcutInput from "./ShortcutInput";
import type { AiStatus } from "../types";
import ThemePicker from "./ThemePicker";

const LAUNCH_ON_STARTUP_KEY = "launch_on_startup";
const DEFAULT_IDE_KEY = "default_ide";
const PYTHON_PATH_KEY = "python_path";
const NODE_PATH_KEY = "node_path";
const POMODORO_MINUTES_KEY = "pomodoro_minutes";
/**
 * Tells the user which imported scripts cannot run here, when any cannot.
 *
 * Worth saying once, clearly, at the moment of import: a setup that came from
 * the other platform keeps every script it had, and the ones belonging to that
 * platform are simply inert. Left unsaid, the user meets them one at a time,
 * and each one looks like a bug in Orion rather than a fact about the OS.
 */
function describeUnrunnable(summary: ImportSummary): string {
  if (summary.unrunnableCount === 0) return "";

  const count = summary.unrunnableCount;
  const noun = count === 1 ? "script" : "scripts";
  const types = summary.unrunnableTypes.join(" and ");
  const named =
    summary.unrunnableScripts.length < count
      ? ""
      : ` (${summary.unrunnableScripts.join(", ")})`;

  return `${count} ${types} ${noun}${named} came across unchanged but cannot run on ${platform().os === "macos" ? "macOS" : "Windows"} — they are kept so they still work on the machine they were written for.`;
}

const SPOTLIGHT_SHORTCUT_KEY = "spotlight_shortcut";
const DEFAULT_SPOTLIGHT_SHORTCUT = "CmdOrCtrl+Shift+O";
const DEFAULT_IDE_FALLBACK = "code";
const DEFAULT_POMODORO_MINUTES = 25;

/**
 * Text input that persists trimmed values via a 500ms debounced onChange,
 * flushes immediately on Enter, and flushes any pending edit on unmount so
 * navigating away can never lose data.
 */
function DebouncedPathInput({
  id,
  label,
  description,
  initial,
  placeholder,
  persist,
  onPersistError,
  disabled,
  maxLength = 260,
}: {
  id: string;
  label: string;
  description?: string;
  initial: string;
  placeholder?: string;
  persist: (value: string) => Promise<unknown>;
  onPersistError: (message: string) => void;
  disabled?: boolean;
  maxLength?: number;
}) {
  const [draft, setDraft] = useState(initial);
  const draftRef = useRef(initial);
  const savedRef = useRef(initial);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    setDraft(initial);
    draftRef.current = initial;
    savedRef.current = initial;
  }, [initial]);

  const persistNow = useCallback(
    (raw: string) => {
      const trimmed = raw.trim();
      if (trimmed === savedRef.current) return;
      const previous = savedRef.current;
      savedRef.current = trimmed;
      Promise.resolve(persist(trimmed)).catch((e) => {
        onPersistError(String(e));
        savedRef.current = previous;
        draftRef.current = previous;
        setDraft(previous);
      });
    },
    [persist, onPersistError]
  );

  const handleChange = useCallback(
    (raw: string) => {
      setDraft(raw);
      draftRef.current = raw;
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        persistNow(raw);
      }, 500);
    },
    [persistNow]
  );

  // Flush pending edits when the settings view unmounts.
  useEffect(
    () => () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        void persistNow(draftRef.current);
      }
    },
    [persistNow]
  );

  return (
    <div>
      <label htmlFor={id} className="block text-[13px] font-medium text-ink">
        {label}
      </label>
      {description && (
        <p className="mt-0.5 mb-2 text-xs leading-relaxed text-ink-faint">
          {description}
        </p>
      )}
      <input
        id={id}
        value={draft}
        onChange={(e) => handleChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            if (timerRef.current !== null) {
              window.clearTimeout(timerRef.current);
              timerRef.current = null;
            }
            persistNow(draftRef.current);
          }
        }}
        maxLength={maxLength}
        disabled={disabled}
        spellCheck={false}
        placeholder={placeholder}
        className="w-full max-w-xs rounded bg-raised px-2.5 py-1.5 font-mono text-[13px] text-ink-strong placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50 disabled:opacity-40"
      />
    </div>
  );
}

interface SettingsViewProps {
  onError: (message: string | null) => void;
  onDataReplaced: () => void;
}

function Toggle({
  checked,
  disabled,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-10 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
        checked ? "bg-accent" : "bg-raised-hover"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full shadow transition-transform ${
          // Not plain white: Almanac's accent is itself near-white, so a white
          // knob on a checked switch vanishes into its own track.
          checked ? "translate-x-5 bg-accent-on" : "bg-ink-strong"
        }`}
      />
    </button>
  );
}

const TABS = ["general", "ai", "data"] as const;
const TAB_LABELS: Record<(typeof TABS)[number], string> = {
  general: "General",
  ai: "AI Search",
  data: "Data",
};

export default function SettingsView({ onError, onDataReplaced }: SettingsViewProps) {
  const [tab, setTab] = useState<(typeof TABS)[number]>("general");

  const [spotlightShortcut, setSpotlightShortcutState] = useState(
    DEFAULT_SPOTLIGHT_SHORTCUT
  );
  const [aiStatus, setAiStatus] = useState<AiStatus | null>(null);
  const [buildingIndex, setBuildingIndex] = useState(false);
  const [removingModel, setRemovingModel] = useState(false);
  const [aiMessage, setAiMessage] = useState<string | null>(null);

  const [startWithWindows, setStartWithWindows] = useState(false);
  const [ideCommand, setIdeCommand] = useState(DEFAULT_IDE_FALLBACK);
  const [pythonPath, setPythonPath] = useState("");
  const [nodePath, setNodePath] = useState("");
  const [pomodoroDraft, setPomodoroDraft] = useState(String(DEFAULT_POMODORO_MINUTES));
  const [settingsLoaded, setSettingsLoaded] = useState(false);

  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const savedPomodoroRef = useRef(DEFAULT_POMODORO_MINUTES);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setStartWithWindows(s[LAUNCH_ON_STARTUP_KEY] === "true");

        setIdeCommand(s[DEFAULT_IDE_KEY]?.trim() || DEFAULT_IDE_FALLBACK);
        setPythonPath(s[PYTHON_PATH_KEY]?.trim() ?? "");
        setNodePath(s[NODE_PATH_KEY]?.trim() ?? "");

        const parsed = Number.parseInt(s[POMODORO_MINUTES_KEY] ?? "", 10);
        const minutes =
          Number.isFinite(parsed) && parsed >= 1 && parsed <= 180
            ? parsed
            : DEFAULT_POMODORO_MINUTES;
        savedPomodoroRef.current = minutes;
        setPomodoroDraft(String(minutes));

        setSpotlightShortcutState(
          s[SPOTLIGHT_SHORTCUT_KEY]?.trim() || DEFAULT_SPOTLIGHT_SHORTCUT
        );
      })
      .catch((e) => onError(String(e)))
      .finally(() => setSettingsLoaded(true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSaveShortcut = useCallback(
    async (accelerator: string) => {
      onError(null);
      // Awaited and deliberately not caught: ShortcutInput surfaces the failure
      // inline, and local state must only advance once the backend confirms the
      // accelerator is actually registered.
      await setSpotlightShortcut(accelerator);
      setSpotlightShortcutState(accelerator);
    },
    [onError]
  );

  const refreshAiStatus = useCallback(() => {
    getAiStatus()
      .then(setAiStatus)
      .catch((e) => onError(String(e)));
  }, [onError]);

  useEffect(() => {
    if (tab === "ai") refreshAiStatus();
  }, [tab, refreshAiStatus]);

  const handleBuildIndex = useCallback(async () => {
    if (buildingIndex) return;
    onError(null);
    setAiMessage(null);
    setBuildingIndex(true);
    try {
      await reindexAll();
      setAiMessage("Semantic search is ready. Type /ai in the search bar to use it.");
    } catch (e) {
      // The download is the most likely failure, and it is resumable — say so
      // rather than leaving the user guessing whether to start over.
      setAiMessage(null);
      onError(`${e}. You can safely press the button again to resume.`);
    } finally {
      setBuildingIndex(false);
      refreshAiStatus();
    }
  }, [buildingIndex, onError, refreshAiStatus]);

  const handleRemoveModel = useCallback(async () => {
    if (removingModel || buildingIndex) return;
    const approved = await confirm(
      "Delete the downloaded model and clear the search index? Your workspaces, links and scripts are not affected, and you can download it again at any time.",
      { title: "Remove Semantic Search", kind: "warning" }
    );
    if (!approved) return;

    onError(null);
    setAiMessage(null);
    setRemovingModel(true);
    try {
      await removeAiModel();
      setAiMessage(
        "Model removed and index cleared. The memory it was using is released when you next restart Orion."
      );
    } catch (e) {
      onError(String(e));
    } finally {
      setRemovingModel(false);
      refreshAiStatus();
    }
  }, [removingModel, buildingIndex, onError, refreshAiStatus]);

  const persistIde = useCallback(
    (value: string) =>
      updateSetting(DEFAULT_IDE_KEY, value || DEFAULT_IDE_FALLBACK),
    []
  );
  const persistPythonPath = useCallback(
    (value: string) => updateSetting(PYTHON_PATH_KEY, value),
    []
  );
  const persistNodePath = useCallback(
    (value: string) => updateSetting(NODE_PATH_KEY, value),
    []
  );

  const persistPomodoro = useCallback(
    (raw: string) => {
      const parsed = Number.parseInt(raw, 10);
      onError(null);
      if (!Number.isFinite(parsed)) {
        setPomodoroDraft(String(savedPomodoroRef.current));
        return;
      }
      const clamped = Math.min(Math.max(parsed, 1), 180);
      setPomodoroDraft(String(clamped));
      if (clamped === savedPomodoroRef.current) return;
      const previous = savedPomodoroRef.current;
      savedPomodoroRef.current = clamped;
      updateSetting(POMODORO_MINUTES_KEY, String(clamped)).catch((e) => {
        onError(String(e));
        savedPomodoroRef.current = previous;
        setPomodoroDraft(String(previous));
      });
    },
    [onError]
  );

  const handleToggleStartup = useCallback(
    (value: boolean) => {
      const previous = startWithWindows;
      setStartWithWindows(value);
      onError(null);
      updateSetting(LAUNCH_ON_STARTUP_KEY, value ? "true" : "false").catch(
        (e) => {
          onError(String(e));
          setStartWithWindows(previous);
        }
      );
    },
    [startWithWindows, onError]
  );

  const handleOpenLogs = useCallback(() => {
    invoke("open_log_folder").catch((e) => onError(String(e)));
  }, [onError]);

  const handleCheckUpdates = useCallback(async () => {
    onError(null);

    // The macOS build is unsigned, so it is deliberately not served by the
    // in-app updater: the download would be quarantined by Gatekeeper and the
    // in-place replacement would fail. Saying so is better than letting the
    // check fail and reporting it as a network problem.
    if (platform().os === "macos") {
      setUpdateStatus(
        "On macOS, update by downloading the latest .dmg from the Releases page — in-app updates need a signed build."
      );
      return;
    }

    setCheckingUpdates(true);
    try {
      const update = await check();
      if (update?.available) {
        setUpdateStatus(`Update ${update.version} is available for download.`);
      } else {
        setUpdateStatus("You are on the latest version.");
      }
    } catch {
      // Expected while the release endpoint is a placeholder.
      setUpdateStatus(
        "Could not reach the update server. You may be on a dev build."
      );
    } finally {
      setCheckingUpdates(false);
    }
  }, [onError]);

  const handleExport = useCallback(async () => {
    onError(null);
    setStatusMessage(null);
    setExporting(true);
    try {
      const filePath = await exportBackupToFile();
      if (filePath) {
        setStatusMessage(`Backup saved to ${filePath}`);
      }
      // null = user cancelled the native dialog — no message needed.
    } catch (e) {
      onError(String(e));
    } finally {
      setExporting(false);
    }
  }, [onError]);

  const confirmImport = useCallback(async () => {
    if (!pendingFile || importing) return;
    setImporting(true);
    try {
      const text = await pendingFile.text();
      const summary = await importData(text);
      setPendingFile(null);
      // An import replaces the whole database, which clears the vector index
      // with it. Say so here rather than letting /ai come back empty later.
      setStatusMessage(
        [
          "Backup imported successfully.",
          describeUnrunnable(summary),
          "If you use semantic search, go to the AI Search tab and rebuild the index — an import clears it.",
        ]
          .filter(Boolean)
          .join(" ")
      );
      onDataReplaced();
    } catch (e) {
      setPendingFile(null);
      onError(String(e));
    } finally {
      setImporting(false);
    }
  }, [pendingFile, importing, onDataReplaced, onError]);

  function onFileChosen(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    onError(null);
    setStatusMessage(null);
    setPendingFile(file);
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2.5 border-b border-rule/80 px-4">
        <h2 className="text-sm font-medium text-ink">Settings</h2>
        <div className="ml-auto flex overflow-hidden rounded-md ring-1 ring-rule-strong">
          {TABS.map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`px-3 py-1 text-xs font-medium transition-colors cursor-default ${
                tab === t
                  ? "bg-accent text-accent-on"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              {TAB_LABELS[t]}
            </button>
          ))}
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        <div className="max-w-xl">
          {tab === "general" && (
            <div className="space-y-2">
              <ThemePicker onError={onError} />
              <div className="flex items-center gap-4 rounded-lg border border-rule bg-surface/70 p-4">
                <div className="min-w-0 flex-1">
                  <p className="text-[13px] font-medium text-ink">
                    Launch on System Startup
                  </p>
                  <p className="mt-0.5 text-xs leading-relaxed text-ink-faint">
                    Start Orion automatically in the background when you sign
                    in to your computer. Only the tray icon will appear.
                  </p>
                </div>
                {!settingsLoaded ? (
                  <Loader2 size={16} className="animate-spin text-ink-faint" />
                ) : (
                  <Toggle
                    checked={startWithWindows}
                    onChange={handleToggleStartup}
                  />
                )}
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <DebouncedPathInput
                  id="default-ide"
                  label="Default IDE Command"
                  description="Used by the folder quick-launch button. Enter a command on your PATH (e.g. code, cursor, subl) or the full path to an executable."
                  initial={ideCommand}
                  persist={persistIde}
                  onPersistError={(m) => onError(m)}
                  disabled={!settingsLoaded}
                />
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  Custom Interpreters
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-ink-faint">
                  Point script execution at specific executables — for example
                  a virtual environment's Python. Leave blank to use the
                  system default.
                </p>
                <div className="space-y-4">
                  <DebouncedPathInput
                    id="python-path"
                    label="Python Executable Path"
                    initial={pythonPath}
                    placeholder={
                      platform().os === "macos"
                        ? "/opt/homebrew/bin/python3"
                        : "C:\\venvs\\main\\Scripts\\python.exe"
                    }
                    persist={persistPythonPath}
                    onPersistError={(m) => onError(m)}
                    disabled={!settingsLoaded}
                  />
                  <DebouncedPathInput
                    id="node-path"
                    label="Node.js Executable Path"
                    initial={nodePath}
                    placeholder={
                      platform().os === "macos"
                        ? "/opt/homebrew/bin/node"
                        : "D:\\tools\\node\\node.exe"
                    }
                    persist={persistNodePath}
                    onPersistError={(m) => onError(m)}
                    disabled={!settingsLoaded}
                  />
                </div>
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <label
                  htmlFor="pomodoro-minutes"
                  className="block text-[13px] font-medium text-ink"
                >
                  Pomodoro Duration (minutes)
                </label>
                <p className="mt-0.5 mb-2 text-xs leading-relaxed text-ink-faint">
                  Countdown length for pomodoro sessions (1–180). Applies to
                  new timers immediately.
                </p>
                <input
                  id="pomodoro-minutes"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={180}
                  value={pomodoroDraft}
                  onChange={(e) => setPomodoroDraft(e.target.value)}
                  onBlur={(e) => persistPomodoro(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      persistPomodoro((e.target as HTMLInputElement).value);
                    }
                  }}
                  disabled={!settingsLoaded}
                  className="w-24 rounded bg-raised px-2.5 py-1.5 text-center text-[13px] tabular-nums text-ink-strong outline-none ring-1 ring-transparent focus:ring-focus/50 disabled:opacity-40"
                />
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  Global Search Shortcut
                </p>
                <p className="mt-0.5 mb-2.5 text-xs leading-relaxed text-ink-faint">
                  Summons the search window from anywhere. If another app
                  already owns the combination, Orion keeps the previous one and
                  tells you.
                </p>
                <ShortcutInput
                  value={spotlightShortcut}
                  onSave={handleSaveShortcut}
                  disabled={!settingsLoaded}
                />
              </div>

              <p className="px-1 pt-1 text-[10px] text-ink-faint">
                Autostart is applied immediately, via{" "}
                {platform().os === "macos"
                  ? "a macOS login item"
                  : "the Windows registry"}
                .
              </p>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  Troubleshooting &amp; Updates
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-ink-faint">
                  Inspect application logs or check the release server for a
                  newer build.
                </p>
                <div className="flex flex-wrap gap-2">
                  <button
                    onClick={handleOpenLogs}
                    className="flex items-center gap-1.5 rounded bg-raised-hover px-3 py-1.5 text-xs font-medium text-ink-strong transition-colors hover:bg-ink-ghost"
                  >
                    <FolderOpen size={13} />
                    Open Log Folder
                  </button>
                  <button
                    onClick={() => void handleCheckUpdates()}
                    disabled={checkingUpdates}
                    className="flex items-center gap-1.5 rounded bg-accent/90 px-3 py-1.5 text-xs font-medium text-accent-on transition-colors enabled:hover:bg-accent-bright disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    {checkingUpdates ? (
                      <Loader2 size={13} className="animate-spin" />
                    ) : (
                      <RefreshCw size={13} />
                    )}
                    Check for Updates
                  </button>
                </div>
                {updateStatus && (
                  <p className="mt-2.5 text-xs leading-relaxed text-ink-muted">
                    {updateStatus}
                  </p>
                )}
              </div>
            </div>
          )}

          {tab === "ai" && (
            <div className="space-y-3">
              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <div className="mb-3 flex items-start gap-3">
                  <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded bg-accent/15 text-accent-ink">
                    <Sparkles size={14} />
                  </span>
                  <div className="min-w-0">
                    <p className="text-[13px] font-medium text-ink">
                      Semantic Search
                    </p>
                    <p className="mt-0.5 text-xs leading-relaxed text-ink-faint">
                      Find things by meaning instead of exact words — searching
                      “deploy my app live” can surface a script called “Ship the
                      release to production”. Type{" "}
                      <span className="font-mono text-ink-muted">/ai</span> in the
                      search bar once this is enabled.
                    </p>
                  </div>
                </div>

                <dl className="mb-3 grid grid-cols-2 gap-2 border-t border-rule pt-3 text-xs">
                  <div>
                    <dt className="text-ink-faint">Model</dt>
                    <dd className="mt-0.5 font-medium">
                      {aiStatus === null ? (
                        <span className="text-ink-faint">Checking…</span>
                      ) : aiStatus.downloaded ? (
                        <span className="text-positive-ink">Ready</span>
                      ) : (
                        <span className="text-ink-muted">Not downloaded</span>
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-ink-faint">On disk</dt>
                    <dd className="mt-0.5 font-medium text-ink-muted">
                      {aiStatus === null
                        ? "—"
                        : aiStatus.modelBytes === 0
                          ? "Nothing cached"
                          : `${(aiStatus.modelBytes / 1024 / 1024).toFixed(0)} MB`}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-ink-faint">Indexed items</dt>
                    <dd className="mt-0.5 font-medium text-ink-muted">
                      {aiStatus === null
                        ? "—"
                        : `${aiStatus.indexedCount} of ${aiStatus.indexableCount}`}
                    </dd>
                  </div>
                </dl>

                {/* Only flagged once the model is present: before that, the
                    empty index is expected rather than out of date. */}
                {aiStatus?.downloaded &&
                  aiStatus.indexedCount < aiStatus.indexableCount && (
                    <p className="mb-3 flex items-start gap-2 rounded border border-caution/30 bg-caution/10 px-2.5 py-2 text-xs leading-relaxed text-caution-ink/90">
                      <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                      <span>
                        {aiStatus.indexableCount - aiStatus.indexedCount} item(s)
                        are not in the index yet and will not appear in{" "}
                        <span className="font-mono">/ai</span> results. Rebuild
                        to include them.
                      </span>
                    </p>
                  )}

                <div className="flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => void handleBuildIndex()}
                    disabled={buildingIndex || removingModel || aiStatus === null}
                  className="flex items-center gap-1.5 rounded bg-accent/90 px-3 py-1.5 text-xs font-medium text-accent-on transition-colors enabled:hover:bg-accent-bright disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {/* Fetching ~90 MB and re-embedding what is already local
                      are different actions and should not share an icon. */}
                  {buildingIndex ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : aiStatus?.downloaded ? (
                    <RefreshCw size={13} />
                  ) : (
                    <Download size={13} />
                  )}
                  {buildingIndex
                    ? "Working…"
                    : aiStatus?.downloaded
                      ? "Rebuild Index"
                      : "Download Model & Build Index (~90MB)"}
                  </button>

                  {/* Only offered once there is something to remove. */}
                  {aiStatus?.downloaded && (
                    <button
                      onClick={() => void handleRemoveModel()}
                      disabled={removingModel || buildingIndex}
                      className="flex items-center gap-1.5 rounded border border-rule-strong px-3 py-1.5 text-xs font-medium text-ink-muted transition-colors enabled:hover:border-danger/50 enabled:hover:text-danger-ink disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {removingModel ? (
                        <Loader2 size={13} className="animate-spin" />
                      ) : (
                        <Trash2 size={13} />
                      )}
                      Remove model
                    </button>
                  )}
                </div>

                {buildingIndex && (
                  <p className="mt-2.5 text-xs leading-relaxed text-ink-muted">
                    {aiStatus?.downloaded
                      ? "Re-embedding every item. You can keep using Orion while it works."
                      : "The one-time ~90 MB download can take several minutes. It resumes automatically if the connection drops, and you can keep using Orion while it works."}
                  </p>
                )}
                {aiMessage && !buildingIndex && (
                  <p className="mt-2.5 flex items-start gap-1.5 text-xs leading-relaxed text-positive-ink">
                    <Check size={13} className="mt-0.5 shrink-0" />
                    {aiMessage}
                  </p>
                )}
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  What leaves your computer
                </p>
                <p className="mt-0.5 text-xs leading-relaxed text-ink-faint">
                  Only the one-time model download itself. The model then runs
                  locally on your CPU — your workspaces, links, scripts and
                  search queries are never uploaded anywhere. Remove model
                  deletes the downloaded weights and clears the index whenever
                  you want the space back.
                </p>
              </div>
            </div>
          )}

          {tab === "data" && (
            <div className="space-y-3">
              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  Export data backup
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-ink-faint">
                  Download all workspaces, resources, scripts and timer history
                  as a single JSON file.
                </p>
                <button
                  onClick={() => void handleExport()}
                  disabled={exporting}
                  className="flex items-center gap-1.5 rounded bg-accent/90 px-3 py-1.5 text-xs font-medium text-accent-on transition-colors enabled:hover:bg-accent-bright disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {exporting ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Download size={13} />
                  )}
                  Export data backup
                </button>
              </div>

              <div className="rounded-lg border border-rule bg-surface/70 p-4">
                <p className="text-[13px] font-medium text-ink">
                  Import data
                </p>
                <p className="mt-0.5 mb-2 text-xs leading-relaxed text-ink-faint">
                  Restore from an orion_backup.json file. Your workspace,
                  resource and script ordering is preserved.
                  <span className="text-caution-ink">
                    {" "}
                    This replaces all existing data.
                  </span>
                </p>
                <p className="mb-3 flex items-start gap-1.5 text-xs leading-relaxed text-ink-faint">
                  <Sparkles size={12} className="mt-0.5 shrink-0 text-accent-ink" />
                  <span>
                    Using semantic search? An import clears the AI index, so
                    rebuild it afterwards in the{" "}
                    <span className="text-ink-muted">AI Search</span> tab.
                  </span>
                </p>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".json,application/json"
                  onChange={onFileChosen}
                  className="hidden"
                />
                <button
                  onClick={() => fileInputRef.current?.click()}
                  disabled={importing}
                  className="flex items-center gap-1.5 rounded bg-raised-hover px-3 py-1.5 text-xs font-medium text-ink-strong transition-colors enabled:hover:bg-ink-ghost disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {importing ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Upload size={13} />
                  )}
                  Import data…
                </button>
              </div>

              {statusMessage && (
                <p className="flex items-center gap-1.5 px-1 text-xs text-positive-ink">
                  <Check size={12} />
                  {statusMessage}
                </p>
              )}
            </div>
          )}
        </div>
      </div>

      {pendingFile && (
        <div
          role="dialog"
          aria-modal="true"
          onKeyDown={(e) => {
            if (e.key === "Escape") setPendingFile(null);
          }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-void/70 backdrop-blur-sm"
        >
          <div className="mx-4 w-full max-w-md rounded-xl border border-rule-strong bg-surface p-5 shadow-2xl">
            <div className="mb-3 flex items-start gap-3">
              <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-danger/15 text-danger-ink">
                <AlertTriangle size={18} />
              </span>
              <div>
                <h3 className="text-sm font-semibold text-ink-strong">
                  Replace all data?
                </h3>
                <p className="mt-1 text-xs leading-relaxed text-ink-muted">
                  Importing{" "}
                  <span className="font-mono text-ink-muted">
                    {pendingFile.name}
                  </span>{" "}
                  will permanently delete your current workspaces, resources,
                  scripts and timer history before restoring the backup. This
                  cannot be undone.
                </p>
              </div>
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={() => setPendingFile(null)}
                className="flex items-center gap-1 rounded bg-raised px-3 py-1.5 text-xs font-medium text-ink-muted transition-colors hover:bg-raised-hover"
              >
                <X size={12} />
                Cancel
              </button>
              <button
                autoFocus
                onClick={() => void confirmImport()}
                disabled={importing}
                className="flex items-center gap-1.5 rounded bg-danger px-3 py-1.5 text-xs font-medium text-danger-on transition-colors enabled:hover:bg-danger disabled:opacity-50"
              >
                {importing && <Loader2 size={12} className="animate-spin" />}
                Replace everything
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
