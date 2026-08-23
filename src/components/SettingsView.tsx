import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  Download,
  FolderOpen,
  Loader2,
  RefreshCw,
  Upload,
  X,
} from "lucide-react";
import { check } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";
import {
  exportBackupToFile,
  getSettings,
  importData,
  updateSetting,
} from "../api/settings";

const LAUNCH_ON_STARTUP_KEY = "launch_on_startup";
const DEFAULT_IDE_KEY = "default_ide";
const PYTHON_PATH_KEY = "python_path";
const NODE_PATH_KEY = "node_path";
const POMODORO_MINUTES_KEY = "pomodoro_minutes";
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

export default function SettingsView({ onError, onDataReplaced }: SettingsViewProps) {
  const [tab, setTab] = useState<"general" | "data">("general");

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
      })
      .catch((e) => onError(String(e)))
      .finally(() => setSettingsLoaded(true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
      setStatusMessage("Backup imported successfully.");
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
          {(["general", "data"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`px-3 py-1 text-xs font-medium transition-colors cursor-default ${
                tab === t
                  ? "bg-indigo-500 text-white"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              {t === "general" ? "General" : "Data"}
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

              <div className="flex items-center justify-between rounded-lg border border-zinc-800 bg-zinc-900/70 p-4">
                <div>
                  <p className="text-[13px] font-medium text-zinc-200">
                    Global search shortcut
                  </p>
                  <p className="mt-0.5 text-xs text-zinc-500">
                    Opens the quick search overlay from anywhere.
                  </p>
                </div>
                <kbd className="rounded bg-zinc-800 px-2 py-1 text-[11px] text-zinc-300">
                  Ctrl+Shift+O
                </kbd>
              </div>

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
