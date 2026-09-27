import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckCircle2, Rocket, Settings as SettingsIcon } from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import ResourcesPanel from "./components/ResourcesPanel";
import SettingsView from "./components/SettingsView";
import Spotlight from "./components/Spotlight";
import TimerAlert from "./components/TimerAlert";
import { notifyPomodoroComplete } from "./utils/notify";
import { useAppVersion } from "./utils/version";
import { logTimerSession } from "./api/timers";
import { getSettings } from "./api/settings";
import {
  createWorkspace,
  deleteWorkspace,
  getWorkspaces,
  reorderWorkspaces,
} from "./api/workspaces";
import type {
  SessionType,
  TimerController,
  TimerData,
  Workspace,
} from "./types";

export default function App() {
  const [label] = useState(() => getCurrentWebviewWindow().label);
  if (label === "spotlight") return <Spotlight />;
  if (label === "alert") return <TimerAlert />;
  return <MainShell />;
}

function MainShell() {
  const appVersion = useAppVersion();
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  // --- Global per-workspace timer state -----------------------------------
  // Lives here so timers keep running (and complete) even when the user is
  // viewing a different workspace and the widget is unmounted.
  const [timers, setTimers] = useState<Record<string, TimerData>>({});
  // Incremented after a session row is committed, so views showing accumulated
  // totals know exactly when a refetch will see the new data.
  const [logVersion, setLogVersion] = useState(0);
  const [pomodoroMinutes, setPomodoroMinutes] = useState(25);
  const [toast, setToast] = useState<{ id: number; message: string } | null>(
    null
  );

  const timersRef = useRef(timers);
  useEffect(() => {
    timersRef.current = timers;
  }, [timers]);

  const pomodoroSeconds = pomodoroMinutes * 60;
  const pomodoroSecondsRef = useRef(pomodoroSeconds);
  useEffect(() => {
    pomodoroSecondsRef.current = pomodoroSeconds;
  }, [pomodoroSeconds]);

  // Pulls app_settings from SQLite so pomodoro_minutes stays in sync with
  // what SettingsView persisted. Runs on mount, and again whenever the user
  // closes the settings view.
  const loadSettings = useCallback(() => {
    getSettings()
      .then((s) => {
        const parsed = Number.parseInt(s["pomodoro_minutes"] ?? "", 10);
        if (Number.isFinite(parsed) && parsed >= 1 && parsed <= 180) {
          setPomodoroMinutes(parsed);
        }
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!settingsOpen) loadSettings();
  }, [settingsOpen, loadSettings]);

  const showToast = useCallback((message: string) => {
    const entry = { id: Date.now(), message };
    setToast(entry);
    window.setTimeout(() => {
      setToast((current) => (current?.id === entry.id ? null : current));
    }, 6000);
  }, []);

  const completePomodoro = useCallback(
    (workspaceId: string) => {
      const timer = timersRef.current[workspaceId];
      if (!timer || timer.phase !== "running") return;

      const total = pomodoroSecondsRef.current;
      setTimers((prev) =>
        prev[workspaceId]
          ? {
              ...prev,
              [workspaceId]: { ...prev[workspaceId], phase: "done", elapsed: total },
            }
          : prev
      );

      void logTimerSession({
        workspaceId,
        durationSeconds: total,
        sessionType: "pomodoro",
      })
        .then(() => setLogVersion((v) => v + 1))
        .catch((e) => setError(String(e)));

      // Beep, Orion's own always-on-top alert window, and a native toast
      // alongside it. Only a failure of the alert window is worth surfacing —
      // it is the one the user is relying on to actually see.
      void notifyPomodoroComplete().then((alertError) => {
        if (alertError) setError(`Timer alert failed: ${alertError}`);
      });
      showToast("Pomodoro Complete! Session logged. Time for a short break.");
    },
    [showToast]
  );

  // Single ticker advances every running timer — including workspaces whose
  // widget is not currently mounted.
  useEffect(() => {
    const interval = window.setInterval(() => {
      const now = Date.now();
      let changed = false;
      const next: Record<string, TimerData> = { ...timersRef.current };

      for (const [id, t] of Object.entries(next)) {
        if (t.phase !== "running" || t.startedAt === null) continue;
        const elapsed = Math.floor((now - t.startedAt) / 1000);
        const capped =
          t.mode === "pomodoro"
            ? Math.min(elapsed, pomodoroSecondsRef.current)
            : elapsed;
        if (capped !== t.elapsed) {
          next[id] = { ...t, elapsed: capped };
          changed = true;
        }
      }

      if (changed) {
        setTimers(next);
        for (const [id, t] of Object.entries(next)) {
          if (
            t.mode === "pomodoro" &&
            t.phase === "running" &&
            t.elapsed >= pomodoroSecondsRef.current
          ) {
            completePomodoro(id);
          }
        }
      }
    }, 250);
    return () => window.clearInterval(interval);
  }, [completePomodoro]);

  const startTimer = useCallback((workspaceId: string, mode: SessionType) => {
    setError(null);
    setTimers((prev) => ({
      ...prev,
      [workspaceId]: {
        mode,
        phase: "running",
        elapsed: 0,
        startedAt: Date.now(),
      },
    }));
  }, []);

  const stopTimer = useCallback((workspaceId: string) => {
    const timer = timersRef.current[workspaceId];
    if (!timer || timer.phase !== "running" || timer.startedAt === null) return;

    const rawElapsed = Math.floor((Date.now() - timer.startedAt) / 1000);
    const duration =
      timer.mode === "pomodoro"
        ? Math.min(rawElapsed, pomodoroSecondsRef.current)
        : rawElapsed;

    setTimers((prev) => ({
      ...prev,
      [workspaceId]: { ...timer, phase: "idle", elapsed: 0, startedAt: null },
    }));

    if (duration >= 1) {
      void logTimerSession({
        workspaceId,
        durationSeconds: duration,
        sessionType: timer.mode,
      })
        .then(() => setLogVersion((v) => v + 1))
        .catch((e) => setError(String(e)));
    }
  }, []);

  const resetTimer = useCallback((workspaceId: string) => {
    setTimers((prev) => ({
      ...prev,
      [workspaceId]: {
        ...(prev[workspaceId] ?? {
          mode: "stopwatch" as SessionType,
        }),
        phase: "idle",
        elapsed: 0,
        startedAt: null,
      },
    }));
  }, []);

  const timer: TimerController = useMemo(
    () => ({
      states: timers,
      pomodoroSeconds,
      logVersion,
      start: startTimer,
      stop: stopTimer,
      reset: resetTimer,
    }),
    [timers, pomodoroSeconds, logVersion, startTimer, stopTimer, resetTimer]
  );
  // -------------------------------------------------------------------------

  const loadWorkspaces = useCallback(() => {
    setLoading(true);
    getWorkspaces()
      .then((list) => {
        setWorkspaces(list);
        setActiveId((current) =>
          current && list.some((w) => w.id === current)
            ? current
            : (list[0]?.id ?? null)
        );
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    loadWorkspaces();
  }, [loadWorkspaces]);

  useEffect(() => {
    // Spotlight can select a workspace from the main window.
    const unlisten = listen<string>("orion-select-workspace", (event) => {
      setActiveId(event.payload);
      setSettingsOpen(false);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const handleCreate = useCallback(
    async (name: string, color: string | null) => {
      setError(null);
      try {
        const ws = await createWorkspace({ name, color });
        setWorkspaces((prev) => [...prev, ws]);
        setActiveId(ws.id);
        setSettingsOpen(false);
      } catch (e) {
        setError(String(e));
      }
    },
    []
  );

  const handleDelete = useCallback(
    (id: string) => {
      setError(null);
      const snapshot = workspaces;
      setWorkspaces((prev) => prev.filter((w) => w.id !== id));
      // Remove the workspace's timer (if any) so the background ticker
      // never keeps advancing an orphaned session.
      setTimers((prev) => {
        if (!(id in prev)) return prev;
        const next = { ...prev };
        delete next[id];
        return next;
      });
      if (activeId === id) {
        const remaining = snapshot.filter((w) => w.id !== id);
        setActiveId(remaining[0]?.id ?? null);
      }
      deleteWorkspace(id).catch((e) => {
        setError(String(e));
        setWorkspaces(snapshot);
      });
    },
    [workspaces, activeId]
  );

  const handleReorder = useCallback(
    (ordered: Workspace[]) => {
      setError(null);
      const snapshot = workspaces;
      // Applied immediately so the drag feels instant, rolled back if the
      // backend rejects the order — the same shape as handleDelete.
      setWorkspaces(ordered);
      reorderWorkspaces(ordered.map((w) => w.id)).catch((e) => {
        setError(String(e));
        setWorkspaces(snapshot);
      });
    },
    [workspaces]
  );

  const handleSelect = useCallback((id: string) => {
    setActiveId(id);
    setSettingsOpen(false);
  }, []);

  const handleDataReplaced = useCallback(() => {
    setActiveId(null);
    setSettingsOpen(true);
    loadWorkspaces();
  }, [loadWorkspaces]);

  const active = workspaces.find((w) => w.id === activeId) ?? null;

  return (
    <div className="flex h-full">
      <Sidebar
        workspaces={workspaces}
        activeId={activeId}
        settingsActive={settingsOpen}
        onSelect={handleSelect}
        onOpenSettings={() => setSettingsOpen((v) => !v)}
        onCreate={handleCreate}
        onDelete={handleDelete}
        onReorder={handleReorder}
      />

      <main className="flex min-w-0 flex-1 flex-col">
        {error && (
          <div className="border-b border-red-900/50 bg-red-950/40 px-4 py-2 text-xs text-red-300">
            {error}
          </div>
        )}

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {loading ? (
            <p className="pt-8 text-center text-sm text-zinc-600">Loading…</p>
          ) : settingsOpen ? (
            <SettingsView onError={setError} onDataReplaced={handleDataReplaced} />
          ) : active ? (
            <ResourcesPanel
              workspace={active}
              onError={setError}
              timer={timer}
              onJumpToWorkspace={handleSelect}
            />
          ) : (
            <div className="flex flex-1 items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-center">
                {workspaces.length > 0 ? (
                  <SettingsIcon size={28} className="text-zinc-700" />
                ) : (
                  <Rocket size={28} className="text-zinc-700" />
                )}
                <p className="text-sm text-zinc-500">
                  {workspaces.length > 0
                    ? "Select a workspace to begin"
                    : "Create your first workspace to begin"}
                </p>
              </div>
            </div>
          )}
        </div>

        <div className="flex h-7 items-center border-t border-zinc-800/80 px-3 text-[10px] text-zinc-600">
          <span>{appVersion ? `Orion v${appVersion}` : "Orion"}</span>
          <span className="ml-auto">{workspaces.length} workspace(s)</span>
        </div>
      </main>

      {toast && (
        <div
          key={toast.id}
          className="fixed bottom-10 right-4 z-50 flex items-center gap-2 rounded-lg border border-emerald-500/40 bg-zinc-900/95 px-3.5 py-2.5 shadow-2xl shadow-black/50 backdrop-blur"
        >
          <CheckCircle2 size={15} className="shrink-0 text-emerald-400" />
          <span className="text-xs leading-snug text-zinc-200">
            {toast.message}
          </span>
        </div>
      )}
    </div>
  );
}
