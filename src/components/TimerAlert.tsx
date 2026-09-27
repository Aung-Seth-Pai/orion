import { useCallback, useEffect, useState } from "react";
import { CheckCircle2, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AlertPayload } from "../types";

/** How long an alert stays up before dismissing itself. */
const AUTO_DISMISS_MS = 8_000;

/**
 * Orion's own timer notification: a compact bar parked where Windows puts its
 * toasts.
 *
 * It exists because a Windows toast can be delivered and still never appear —
 * suppressed by Focus Assist or a per-app setting, with no way for the app to
 * detect it. A window Orion owns cannot be suppressed that way.
 *
 * Kept to one line deliberately. This interrupts whatever the user is doing, so
 * it has to be readable at a glance and gone again; a large card in the corner
 * of the screen reads as a problem to deal with rather than a nudge.
 */
export default function TimerAlert() {
  const [alert, setAlert] = useState<AlertPayload | null>(null);

  const dismiss = useCallback(() => {
    setAlert(null);
    void invoke("hide_timer_alert").catch(() => {});
  }, []);

  // index.html and index.css give html/body an opaque zinc background, which is
  // right for every other window but would put a dark square behind this one's
  // rounded bar. All windows share one document, so the override has to happen
  // here at runtime rather than in the stylesheet.
  useEffect(() => {
    const { documentElement, body } = document;
    documentElement.style.backgroundColor = "transparent";
    body.style.backgroundColor = "transparent";
  }, []);

  // Asked for on mount, because an event emitted before this listener attached
  // would be lost and leave a visible but blank window.
  useEffect(() => {
    let cancelled = false;
    invoke<AlertPayload | null>("get_pending_alert")
      .then((pending) => {
        if (!cancelled && pending) setAlert(pending);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // And listened for, so a second alert swaps the contents of an already-open
  // window instead of needing a re-mount.
  useEffect(() => {
    const unlisten = listen<AlertPayload>("orion-timer-alert", (event) => {
      setAlert(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Keyed on nonce, not on the text: two identical back-to-back pomodoros must
  // restart the countdown rather than let the first one's timer dismiss the
  // second.
  useEffect(() => {
    if (!alert) return;
    const timer = window.setTimeout(dismiss, AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [alert?.nonce, alert, dismiss]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") dismiss();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  if (!alert) return null;

  return (
    <div
      onClick={dismiss}
      title="Click to dismiss"
      className="group flex h-full w-full cursor-default items-center gap-2.5 rounded-lg border border-zinc-700/80 bg-zinc-900/95 px-3 shadow-lg shadow-black/50 backdrop-blur"
    >
      <CheckCircle2 size={15} className="shrink-0 text-emerald-400" />

      <div className="flex min-w-0 flex-1 items-baseline gap-1.5">
        <span className="shrink-0 text-xs font-semibold text-zinc-100">
          {alert.title}
        </span>
        <span className="truncate text-[11px] text-zinc-400">{alert.body}</span>
      </div>

      <span
        role="button"
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation();
          dismiss();
        }}
        title="Dismiss"
        className="shrink-0 rounded p-0.5 text-zinc-600 opacity-0 transition-opacity group-hover:opacity-100 hover:text-zinc-300"
      >
        <X size={13} />
      </span>
    </div>
  );
}
