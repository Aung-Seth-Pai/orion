import { useCallback, useEffect, useState } from "react";
import { CheckCircle2, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AlertPayload } from "../types";

/** How long an alert stays up before dismissing itself. */
const AUTO_DISMISS_MS = 10_000;

/**
 * Orion's own timer notification, rendered in a frameless always-on-top
 * window. This exists because a Windows toast can be delivered and still never
 * appear — suppressed by Focus Assist or a per-app setting, with no way for the
 * app to detect it. A window Orion owns cannot be suppressed that way.
 */
export default function TimerAlert() {
  const [alert, setAlert] = useState<AlertPayload | null>(null);

  const dismiss = useCallback(() => {
    setAlert(null);
    void invoke("hide_timer_alert").catch(() => {});
  }, []);

  // index.html and index.css give html/body an opaque zinc background, which is
  // right for every other window but would put a dark square behind this one's
  // rounded card. All windows share one document, so the override has to happen
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
      className="flex h-full w-full cursor-default items-start gap-3 rounded-xl border border-emerald-500/40 bg-zinc-900/95 p-4 shadow-2xl shadow-black/60 backdrop-blur"
    >
      <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-400">
        <CheckCircle2 size={17} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-semibold text-zinc-100">
          {alert.title}
        </p>
        <p className="mt-1 text-xs leading-relaxed text-zinc-400">
          {alert.body}
        </p>
      </div>
      <span
        role="button"
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation();
          dismiss();
        }}
        className="-mt-1 -mr-1 shrink-0 rounded p-1 text-zinc-600 transition-colors hover:text-zinc-300"
      >
        <X size={14} />
      </span>
    </div>
  );
}
