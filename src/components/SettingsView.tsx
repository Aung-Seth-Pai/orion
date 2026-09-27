import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  BellRing,
  Check,
  Download,
  FolderOpen,
  Loader2,
  RefreshCw,
  Sparkles,
  Upload,
  X,
} from "lucide-react";
import { check } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";
import {
  exportBackupToFile,
  getSettings,
  importData,
  setSpotlightShortcut,
  updateSetting,
} from "../api/settings";
import { getAiStatus, reindexAll } from "../api/ai";
import { playBeep, sendNativeToast, showTimerAlert } from "../utils/notify";
import ShortcutInput from "./ShortcutInput";
import type { AiStatus } from "../types";

const LAUNCH_ON_STARTUP_KEY = "launch_on_startup";
const DEFAULT_IDE_KEY = "default_ide";
const PYTHON_PATH_KEY = "python_path";
const NODE_PATH_KEY = "node_path";
const POMODORO_MINUTES_KEY = "pomodoro_minutes";
const SPOTLIGHT_SHORTCUT_KEY = "spotlight_shortcut";
/**
 * The notification self-test is a development diagnostic, not something a user
 * has any reason to run. Vite substitutes this at build time, so the block below
 * is eliminated from the production bundle rather than merely hidden.
 */
const SHOW_NOTIFICATION_DIAGNOSTIC = import.meta.env.DEV;
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
      <label htmlFor={id} className="block text-[13px] font-medium text-zinc-200">
        {label}
      </label>
      {description && (
        <p className="mt-0.5 mb-2 text-xs leading-relaxed text-zinc-500">
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
        className="w-full max-w-xs rounded bg-zinc-800 px-2.5 py-1.5 font-mono text-[13px] text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50 disabled:opacity-40"
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
        checked ? "bg-indigo-500" : "bg-zinc-700"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
          checked ? "translate-x-5" : ""
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
  const [testingNotification, setTestingNotification] = useState(false);
  // null in a field means "that channel worked"; a string is its failure reason.
  const [notifyReport, setNotifyReport] = useState<{
    alert: string | null;
    toast: string | null;
  } | null>(null);

  const [aiStatus, setAiStatus] = useState<AiStatus | null>(null);
  const [buildingIndex, setBuildingIndex] = useState(false);
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

  /// Fires both notification channels and reports each independently, so a
  /// silent timer can be attributed to the right one instead of guessed at.
  const handleTestNotification = useCallback(async () => {
    if (testingNotification) return;
    onError(null);
    setNotifyReport(null);
    setTestingNotification(true);
    try {
      // The real completion path beeps too; a test that did not would look
      // silent and send us chasing the wrong thing.
      playBeep();
      const [alert, toast] = await Promise.all([
        showTimerAlert("Orion test", "If you can read this, timer alerts work."),
        sendNativeToast("Orion test", "If you can read this, Windows notifications work."),
      ]);
      setNotifyReport({ alert, toast });
    } finally {
      setTestingNotification(false);
    }
  }, [testingNotification, onError]);

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
      await importData(text);
      setPendingFile(null);
      // An import replaces the whole database, which clears the vector index
      // with it. Say so here rather than letting /ai come back empty later.
      setStatusMessage(
        "Backup imported successfully. If you use semantic search, rebuild the index in the AI Search tab."
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
      <header className="flex h-12 shrink-0 items-center gap-2.5 border-b border-zinc-800/80 px-4">
        <h2 className="text-sm font-medium text-zinc-200">Settings</h2>
        <div className="ml-auto flex overflow-hidden rounded-md ring-1 ring-zinc-700">
          {TABS.map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`px-3 py-1 text-xs font-medium transition-colors cursor-default ${
                tab === t
                  ? "bg-indigo-500 text-white"
                  : "text-zinc-400 hover:text-zinc-200"
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
              <div className="flex items-center gap-4 rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <div className="min-w-0 flex-1">
                  <p className="text-[13px] font-medium text-zinc-200">
                    Launch on System Startup
                  </p>
                  <p className="mt-0.5 text-xs leading-relaxed text-zinc-500">
                    Start Orion automatically in the background when you sign
                    in to your computer. Only the tray icon will appear.
                  </p>
                </div>
                {!settingsLoaded ? (
                  <Loader2 size={16} className="animate-spin text-zinc-600" />
                ) : (
                  <Toggle
                    checked={startWithWindows}
                    onChange={handleToggleStartup}
                  />
                )}
              </div>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
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

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Custom Interpreters
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-zinc-500">
                  Point script execution at specific executables — for example
                  a virtual environment's Python. Leave blank to use the
                  system default.
                </p>
                <div className="space-y-4">
                  <DebouncedPathInput
                    id="python-path"
                    label="Python Executable Path"
                    initial={pythonPath}
                    placeholder="C:\venvs\main\Scripts\python.exe"
                    persist={persistPythonPath}
                    onPersistError={(m) => onError(m)}
                    disabled={!settingsLoaded}
                  />
                  <DebouncedPathInput
                    id="node-path"
                    label="Node.js Executable Path"
                    initial={nodePath}
                    placeholder="D:\tools\node\node.exe"
                    persist={persistNodePath}
                    onPersistError={(m) => onError(m)}
                    disabled={!settingsLoaded}
                  />
                </div>
              </div>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <label
                  htmlFor="pomodoro-minutes"
                  className="block text-[13px] font-medium text-zinc-200"
                >
                  Pomodoro Duration (minutes)
                </label>
                <p className="mt-0.5 mb-2 text-xs leading-relaxed text-zinc-500">
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
                  className="w-24 rounded bg-zinc-800 px-2.5 py-1.5 text-center text-[13px] tabular-nums text-zinc-100 outline-none ring-1 ring-transparent focus:ring-indigo-500/50 disabled:opacity-40"
                />
              </div>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Global Search Shortcut
                </p>
                <p className="mt-0.5 mb-2.5 text-xs leading-relaxed text-zinc-500">
                  Summons the search window from anywhere in Windows. If another
                  app already owns the combination, Orion keeps the previous one
                  and tells you.
                </p>
                <ShortcutInput
                  value={spotlightShortcut}
                  onSave={handleSaveShortcut}
                  disabled={!settingsLoaded}
                />
              </div>

              {SHOW_NOTIFICATION_DIAGNOSTIC && (
              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Timer Notifications <span className="text-zinc-600">(dev only)</span>
                </p>
                <p className="mt-0.5 mb-2.5 text-xs leading-relaxed text-zinc-500">
                  When a pomodoro finishes, Orion shows its own pop-up window so
                  it cannot be hidden by Windows notification settings, and also
                  sends a Windows notification so it lands in your notification
                  centre. Send a test to check both.
                </p>
                <button
                  onClick={() => void handleTestNotification()}
                  disabled={testingNotification}
                  className="flex items-center gap-1.5 rounded border border-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-300 transition-colors enabled:hover:border-zinc-600 enabled:hover:text-zinc-100 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {testingNotification ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <BellRing size={13} />
                  )}
                  Send test notification
                </button>

                {notifyReport && (
                  <div className="mt-2.5 space-y-1.5 text-xs leading-relaxed">
                    <p
                      className={
                        notifyReport.alert === null
                          ? "flex items-start gap-1.5 text-emerald-400"
                          : "flex items-start gap-1.5 text-red-400"
                      }
                    >
                      {notifyReport.alert === null ? (
                        <Check size={13} className="mt-0.5 shrink-0" />
                      ) : (
                        <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                      )}
                      <span>
                        {notifyReport.alert === null
                          ? "Orion pop-up shown."
                          : `Orion pop-up failed: ${notifyReport.alert}`}
                      </span>
                    </p>
                    <p
                      className={
                        notifyReport.toast === null
                          ? "flex items-start gap-1.5 text-emerald-400"
                          : "flex items-start gap-1.5 text-amber-300"
                      }
                    >
                      {notifyReport.toast === null ? (
                        <Check size={13} className="mt-0.5 shrink-0" />
                      ) : (
                        <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                      )}
                      <span>
                        {notifyReport.toast === null
                          ? "Windows accepted the notification."
                          : `Windows notification failed: ${notifyReport.toast}`}
                      </span>
                    </p>
                    {notifyReport.toast === null && (
                      <p className="text-zinc-500">
                        If you heard a sound but saw no Windows banner, the
                        notification was delivered and only the banner is
                        suppressed. Check Windows Settings &rarr; System &rarr;
                        Notifications &rarr; Orion, and press Win+N to see
                        whether it is waiting in the notification centre. The
                        Orion pop-up above is unaffected by those settings.
                      </p>
                    )}
                  </div>
                )}
              </div>
              )}

              <p className="px-1 pt-1 text-[10px] text-zinc-600">
                Autostart is applied immediately via the Windows registry.
              </p>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Troubleshooting &amp; Updates
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-zinc-500">
                  Inspect application logs or check the release server for a
                  newer build.
                </p>
                <div className="flex flex-wrap gap-2">
                  <button
                    onClick={handleOpenLogs}
                    className="flex items-center gap-1.5 rounded bg-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-100 transition-colors hover:bg-zinc-600"
                  >
                    <FolderOpen size={13} />
                    Open Log Folder
                  </button>
                  <button
                    onClick={() => void handleCheckUpdates()}
                    disabled={checkingUpdates}
                    className="flex items-center gap-1.5 rounded bg-indigo-500/90 px-3 py-1.5 text-xs font-medium text-white transition-colors enabled:hover:bg-indigo-400 disabled:cursor-not-allowed disabled:opacity-40"
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
                  <p className="mt-2.5 text-xs leading-relaxed text-zinc-400">
                    {updateStatus}
                  </p>
                )}
              </div>
            </div>
          )}

          {tab === "ai" && (
            <div className="space-y-3">
              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <div className="mb-3 flex items-start gap-3">
                  <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded bg-indigo-500/15 text-indigo-400">
                    <Sparkles size={14} />
                  </span>
                  <div className="min-w-0">
                    <p className="text-[13px] font-medium text-zinc-200">
                      Semantic Search
                    </p>
                    <p className="mt-0.5 text-xs leading-relaxed text-zinc-500">
                      Find things by meaning instead of exact words — searching
                      “deploy my app live” can surface a script called “Ship the
                      release to production”. Type{" "}
                      <span className="font-mono text-zinc-400">/ai</span> in the
                      search bar once this is enabled.
                    </p>
                  </div>
                </div>

                <dl className="mb-3 grid grid-cols-2 gap-2 border-t border-zinc-800 pt-3 text-xs">
                  <div>
                    <dt className="text-zinc-500">Model</dt>
                    <dd className="mt-0.5 font-medium">
                      {aiStatus === null ? (
                        <span className="text-zinc-600">Checking…</span>
                      ) : aiStatus.downloaded ? (
                        <span className="text-emerald-400">Ready</span>
                      ) : (
                        <span className="text-zinc-400">Not downloaded</span>
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Indexed items</dt>
                    <dd className="mt-0.5 font-medium text-zinc-300">
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
                    <p className="mb-3 flex items-start gap-2 rounded border border-amber-500/30 bg-amber-500/10 px-2.5 py-2 text-xs leading-relaxed text-amber-200/90">
                      <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                      <span>
                        {aiStatus.indexableCount - aiStatus.indexedCount} item(s)
                        are not in the index yet and will not appear in{" "}
                        <span className="font-mono">/ai</span> results. Rebuild
                        to include them.
                      </span>
                    </p>
                  )}

                <button
                  onClick={() => void handleBuildIndex()}
                  disabled={buildingIndex || aiStatus === null}
                  className="flex items-center gap-1.5 rounded bg-indigo-500/90 px-3 py-1.5 text-xs font-medium text-white transition-colors enabled:hover:bg-indigo-400 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {buildingIndex ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Download size={13} />
                  )}
                  {buildingIndex
                    ? "Working…"
                    : aiStatus?.downloaded
                      ? "Rebuild Index"
                      : "Download Model & Build Index (~90MB)"}
                </button>

                {buildingIndex && (
                  <p className="mt-2.5 text-xs leading-relaxed text-zinc-400">
                    Downloading the model on first run can take several minutes.
                    It resumes automatically if the connection drops, and you can
                    keep using Orion while it works.
                  </p>
                )}
                {aiMessage && !buildingIndex && (
                  <p className="mt-2.5 flex items-start gap-1.5 text-xs leading-relaxed text-emerald-400">
                    <Check size={13} className="mt-0.5 shrink-0" />
                    {aiMessage}
                  </p>
                )}
              </div>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  What leaves your computer
                </p>
                <p className="mt-0.5 text-xs leading-relaxed text-zinc-500">
                  Only the one-time model download itself. The model then runs
                  locally on your CPU — your workspaces, links, scripts and
                  search queries are never uploaded anywhere. Deleting Orion's
                  app data directory removes the downloaded model along with it.
                </p>
              </div>
            </div>
          )}

          {tab === "data" && (
            <div className="space-y-3">
              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Export data backup
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-zinc-500">
                  Download all workspaces, resources, scripts and timer history
                  as a single JSON file.
                </p>
                <button
                  onClick={() => void handleExport()}
                  disabled={exporting}
                  className="flex items-center gap-1.5 rounded bg-indigo-500/90 px-3 py-1.5 text-xs font-medium text-white transition-colors enabled:hover:bg-indigo-400 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {exporting ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Download size={13} />
                  )}
                  Export data backup
                </button>
              </div>

              <div className="rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <p className="text-[13px] font-medium text-zinc-200">
                  Import data
                </p>
                <p className="mt-0.5 mb-3 text-xs leading-relaxed text-zinc-500">
                  Restore from an orion_backup.json file.
                  <span className="text-yellow-500/90">
                    {" "}
                    This replaces all existing data.
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
                  className="flex items-center gap-1.5 rounded bg-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-100 transition-colors enabled:hover:bg-zinc-600 disabled:cursor-not-allowed disabled:opacity-40"
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
                <p className="flex items-center gap-1.5 px-1 text-xs text-emerald-400">
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
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
        >
          <div className="mx-4 w-full max-w-md rounded-xl border border-zinc-700 bg-zinc-900 p-5 shadow-2xl">
            <div className="mb-3 flex items-start gap-3">
              <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-red-500/15 text-red-400">
                <AlertTriangle size={18} />
              </span>
              <div>
                <h3 className="text-sm font-semibold text-zinc-100">
                  Replace all data?
                </h3>
                <p className="mt-1 text-xs leading-relaxed text-zinc-400">
                  Importing{" "}
                  <span className="font-mono text-zinc-300">
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
                className="flex items-center gap-1 rounded bg-zinc-800 px-3 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:bg-zinc-700"
              >
                <X size={12} />
                Cancel
              </button>
              <button
                autoFocus
                onClick={() => void confirmImport()}
                disabled={importing}
                className="flex items-center gap-1.5 rounded bg-red-600 px-3 py-1.5 text-xs font-medium text-white transition-colors enabled:hover:bg-red-500 disabled:opacity-50"
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
