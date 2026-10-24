import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckCircle2, Rocket, Settings as SettingsIcon } from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import ResourcesPanel from "./components/ResourcesPanel";
import SettingsView from "./components/SettingsView";
import PomodoroHairline from "./components/PomodoroHairline";
import Spotlight from "./components/Spotlight";
import TimerAlert from "./components/TimerAlert";
import { notifyPomodoroComplete } from "./utils/notify";
import { useAppVersion } from "./utils/version";
import { getAllWorkspaceTimes, logTimerSession } from "./api/timers";
import { getSettings } from "./api/settings";
import {
  applyTheme,
  isThemeId,
  THEME_SETTING_KEY,
} from "./utils/theme";
import {
  createWorkspace,
  deleteWorkspace,
  getWorkspaces,
  reorderWorkspaces,
  updateWorkspace,
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
  const [workspaceTimes, setWorkspaceTimes] = useState<Record<string, number>>(
    {}
  );
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

        // The theme was already painted from the localStorage cache before
        // React mounted. This reconciles it with the stored value, which
        // matters after importing a backup made on another machine. An absent
        // or unrecognised value leaves the cached theme alone rather than
        // yanking the user back to the default.
        const storedTheme = s[THEME_SETTING_KEY];
        if (isThemeId(storedTheme)) applyTheme(storedTheme);
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
      markChanged: () => setLogVersion((v) => v + 1),
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
        // A successful load makes any earlier load failure stale. Without this
        // the banner outlived the problem: a startup error sat there until some
        // unrelated action happened to clear it, which made a transient failure
        // look permanent.
        setError(null);
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

  // Every workspace's accumulated time in one call, for the sidebar's
  // constellation marks. Batched deliberately: the per-workspace command would
  // be an N+1 across the whole list, re-run on every logged session.
  //
  // Refetched when a session is committed or a total is reset (logVersion) and
  // when the list itself changes. A failure is silent -- the marks fall back to
  // their floor, which reads as a new workspace rather than as an error, and
  // there is nothing the user could do about it anyway.
  useEffect(() => {
    let cancelled = false;
    getAllWorkspaceTimes()
      .then((times) => {
        if (!cancelled) setWorkspaceTimes(times);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [logVersion, workspaces]);

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

  const handleRename = useCallback(
    (id: string, name: string) => {
      setError(null);
      const snapshot = workspaces;
      // Applied immediately and rolled back if the backend refuses -- the same
      // shape as handleDelete and handleReorder.
      setWorkspaces((prev) =>
        prev.map((w) => (w.id === id ? { ...w, name } : w))
      );
      updateWorkspace(id, { name }).catch((e) => {
        setError(String(e));
        setWorkspaces(snapshot);
      });
    },
    [workspaces]
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

  // Only the active workspace's pomodoro, and only while it is running. Timers
  // are per-workspace and more than one can be going, but a bar on the window
  // edge has no way to say WHICH context it belongs to -- so it tracks the one
  // the user is looking at, or it says nothing.
  const activeTimer = activeId ? timers[activeId] : undefined;
  const pomodoroFraction =
    activeTimer &&
    activeTimer.mode === "pomodoro" &&
    activeTimer.phase === "running" &&
    pomodoroSeconds > 0
      ? activeTimer.elapsed / pomodoroSeconds
      : null;

  return (
    // relative so the pomodoro bar can span the whole window edge rather than
    // starting after the sidebar, which both reads better and is the point --
    // time passing belongs to the session, not to one panel of it.
    <div className="relative flex h-full">
      <PomodoroHairline fraction={pomodoroFraction} />

      <Sidebar
        workspaces={workspaces}
        workspaceTimes={workspaceTimes}
        activeId={activeId}
        settingsActive={settingsOpen}
        onSelect={handleSelect}
        onOpenSettings={() => setSettingsOpen((v) => !v)}
        onCreate={handleCreate}
        onRename={handleRename}
        onDelete={handleDelete}
        onReorder={handleReorder}
      />

      <main className="flex min-w-0 flex-1 flex-col">

        {error && (
          <div className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-xs text-danger-ink">
            {error}
          </div>
        )}

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {loading ? (
            <p className="pt-8 text-center text-sm text-ink-faint">Loading…</p>
          ) : settingsOpen ? (
            <SettingsView onError={setError} onDataReplaced={handleDataReplaced} />
          ) : active ? (
            // Keyed on the workspace so the settle replays on every switch --
            // and so the panel genuinely starts fresh, rather than carrying the
            // previous context's half-open composer across the threshold.
            <div
              key={active.id}
              className="orion-settle flex min-h-0 flex-1 flex-col"
            >
              <ResourcesPanel
                workspace={active}
                onError={setError}
                timer={timer}
                onJumpToWorkspace={handleSelect}
              />
            </div>
          ) : (
            <div className="flex flex-1 items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-center">
                {workspaces.length > 0 ? (
                  <SettingsIcon size={28} className="text-ink-ghost" />
                ) : (
                  <Rocket size={28} className="text-ink-ghost" />
                )}
                <p className="text-sm text-ink-faint">
                  {workspaces.length > 0
                    ? "Select a workspace to begin"
                    : "Create your first workspace to begin"}
                </p>
              </div>
            </div>
          )}
        </div>

        <div className="flex h-7 items-center border-t border-rule/80 px-3 text-[10px] text-ink-faint">
          <span>{appVersion ? `Orion v${appVersion}` : "Orion"}</span>
          <span className="ml-auto">{workspaces.length} workspace(s)</span>
        </div>
      </main>

      {toast && (
        <div
          key={toast.id}
          className="fixed bottom-10 right-4 z-50 flex items-center gap-2 rounded-lg border border-positive/40 bg-surface/95 px-3.5 py-2.5 shadow-2xl shadow-black/50 backdrop-blur"
        >
          <CheckCircle2 size={15} className="shrink-0 text-positive-ink" />
          <span className="text-xs leading-snug text-ink">
            {toast.message}
          </span>
        </div>
      )}
    </div>
  );
}
