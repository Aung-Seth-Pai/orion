import { Play, Square, Timer as TimerIcon } from "lucide-react";
import type { SessionType, TimerData } from "../types";

interface TimerWidgetProps {
  timer: TimerData;
  pomodoroSeconds: number;
  onStart: (mode: SessionType) => void;
  onStop: () => void;
  onReset: () => void;
}

function formatTime(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export default function TimerWidget({
  timer,
  pomodoroSeconds,
  onStart,
  onStop,
  onReset,
}: TimerWidgetProps) {
  const { mode, phase, elapsed } = timer;
  const running = phase === "running";

  const display =
    mode === "stopwatch"
      ? formatTime(elapsed)
      : phase === "done"
        ? `${formatTime(pomodoroSeconds)} ✓`
        : formatTime(Math.max(pomodoroSeconds - elapsed, 0));

  return (
    <div className="flex items-center gap-1.5 rounded-md bg-zinc-800/70 px-2 py-1 ring-1 ring-zinc-700/50">
      <TimerIcon
        size={13}
        className={running ? "text-emerald-400" : "text-zinc-500"}
      />
      <div className="flex overflow-hidden rounded ring-1 ring-zinc-700">
        {(["stopwatch", "pomodoro"] as SessionType[]).map((m) => (
          <button
            key={m}
            onClick={() => onStart(m)}
            disabled={running}
            title={
              running
                ? "Stop the current session to switch modes"
                : `Switch to ${m === "stopwatch" ? "stopwatch" : "pomodoro"} and start`
            }
            className={`px-1.5 py-0.5 text-[10px] font-medium transition-colors disabled:cursor-not-allowed ${
              mode === m
                ? "bg-zinc-600 text-zinc-100"
                : "bg-transparent text-zinc-500 hover:text-zinc-300"
            }`}
          >
            {m === "stopwatch" ? "SW" : `${Math.round(pomodoroSeconds / 60)}m`}
          </button>
        ))}
      </div>
      <span className="min-w-[3.5rem] text-center font-mono text-xs tabular-nums text-zinc-200">
        {display}
      </span>
      {running ? (
        <button
          onClick={onStop}
          title="Stop and log session"
          className="rounded p-1 text-red-400 transition-colors hover:bg-red-500/10"
        >
          <Square size={11} />
        </button>
      ) : (
        <button
          onClick={() => (phase === "done" ? onReset() : onStart(mode))}
          title={
            phase === "done"
              ? "Pomodoro complete — click to reset"
              : `Start ${mode === "stopwatch" ? "stopwatch" : `${Math.round(pomodoroSeconds / 60)}-minute pomodoro`}`
          }
          className={`rounded p-1 transition-colors ${
            phase === "done"
              ? "text-emerald-400 hover:bg-emerald-500/20"
              : "text-emerald-400 hover:bg-emerald-500/10"
          }`}
        >
          <Play size={11} />
        </button>
      )}
    </div>
  );
}
