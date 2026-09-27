import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Keyboard, Loader2, X } from "lucide-react";

/** Modifier-only presses, which are never a shortcut on their own. */
const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta"]);

/**
 * Turns a keydown into the accelerator string Tauri's global-shortcut parser
 * expects, e.g. `CmdOrCtrl+Shift+O`.
 *
 * Returns null while only modifiers are held, so the field shows the partial
 * combination instead of committing a meaningless one.
 */
function toAccelerator(e: React.KeyboardEvent): string | null {
  const parts: string[] = [];
  // CmdOrCtrl rather than Control so the same stored value works unchanged if
  // this ever runs on macOS.
  if (e.ctrlKey || e.metaKey) parts.push("CmdOrCtrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");

  if (MODIFIER_KEYS.has(e.key)) return null;

  // e.code is layout-independent and gives the physical key, which is what a
  // global shortcut binds to; e.key would give "ø" for Alt+O on some layouts.
  let key = e.code;
  if (key.startsWith("Key")) key = key.slice(3);
  else if (key.startsWith("Digit")) key = key.slice(5);
  else if (key.startsWith("Numpad")) key = `Num${key.slice(6)}`;
  if (!key) return null;

  parts.push(key);
  return parts.join("+");
}

/** Renders `CmdOrCtrl+Shift+O` as something readable on this platform. */
function prettify(accelerator: string): string {
  return accelerator
    .split("+")
    .map((part) => (part === "CmdOrCtrl" ? "Ctrl" : part))
    .join(" + ");
}

interface ShortcutInputProps {
  /** The currently saved accelerator. */
  value: string;
  /** Persists a new accelerator. Rejecting means it could not be registered. */
  onSave: (accelerator: string) => Promise<void>;
  disabled?: boolean;
}

/**
 * Captures a key combination for the global Spotlight shortcut.
 *
 * Recording is explicit — click, press the combination, done — rather than the
 * field listening all the time, which would hijack ordinary typing and Escape.
 */
export default function ShortcutInput({
  value,
  onSave,
  disabled,
}: ShortcutInputProps) {
  const [recording, setRecording] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);

  const stop = useCallback(() => {
    setRecording(false);
    setPending(null);
  }, []);

  useEffect(() => {
    if (recording) buttonRef.current?.focus();
  }, [recording]);

  const commit = useCallback(
    async (accelerator: string) => {
      setSaving(true);
      setError(null);
      try {
        await onSave(accelerator);
        setSaved(true);
        window.setTimeout(() => setSaved(false), 1600);
        stop();
      } catch (e) {
        // Most often the combination is already owned by another application.
        setError(String(e));
        setPending(null);
      } finally {
        setSaving(false);
      }
    },
    [onSave, stop]
  );

  function onKeyDown(e: React.KeyboardEvent) {
    if (!recording) return;
    // Every key is ours while recording, including Tab and Escape — otherwise
    // they would move focus or close the settings view mid-capture.
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      stop();
      return;
    }

    const accelerator = toAccelerator(e);
    if (!accelerator) {
      // Only modifiers so far: show the partial combination as feedback.
      const held: string[] = [];
      if (e.ctrlKey || e.metaKey) held.push("CmdOrCtrl");
      if (e.altKey) held.push("Alt");
      if (e.shiftKey) held.push("Shift");
      setPending(held.length ? held.join("+") : null);
      return;
    }

    // A bare letter would be captured globally and make the keyboard unusable
    // everywhere else, so require at least one modifier.
    if (!accelerator.includes("+")) {
      setError("Use at least one modifier, such as Ctrl or Alt.");
      return;
    }

    setPending(accelerator);
    void commit(accelerator);
  }

  const shown = pending ?? value;

  return (
    <div>
      <div className="flex items-center gap-2">
        <button
          ref={buttonRef}
          onClick={() => (recording ? stop() : setRecording(true))}
          onKeyDown={onKeyDown}
          onBlur={stop}
          disabled={disabled || saving}
          className={`flex min-w-44 items-center justify-center gap-1.5 rounded px-3 py-1.5 font-mono text-xs transition-colors ${
            recording
              ? "bg-indigo-500/15 text-indigo-300 ring-1 ring-indigo-500/60"
              : "bg-zinc-800 text-zinc-200 ring-1 ring-transparent enabled:hover:ring-zinc-700"
          } disabled:cursor-not-allowed disabled:opacity-40`}
        >
          {saving ? (
            <Loader2 size={13} className="animate-spin" />
          ) : (
            <Keyboard size={13} className="opacity-60" />
          )}
          {recording
            ? pending
              ? prettify(pending)
              : "Press keys…"
            : prettify(shown)}
        </button>

        {recording && (
          <button
            onClick={stop}
            title="Cancel (Esc)"
            className="rounded p-1 text-zinc-500 transition-colors hover:text-zinc-300"
          >
            <X size={13} />
          </button>
        )}
        {saved && !recording && (
          <span className="flex items-center gap-1 text-xs text-emerald-400">
            <Check size={13} />
            Saved
          </span>
        )}
      </div>

      {recording && !error && (
        <p className="mt-1.5 text-[11px] text-zinc-500">
          Hold a modifier and press a key. Esc cancels.
        </p>
      )}
      {error && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-red-400">
          {error}
        </p>
      )}
    </div>
  );
}
