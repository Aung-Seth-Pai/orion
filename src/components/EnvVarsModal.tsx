import { useEffect, useState } from "react";
import { KeyRound, Loader2, Plus, X } from "lucide-react";
import { deleteEnvVar, getEnvVars, setEnvVar } from "../api/envvars";
import type { EnvVar, Workspace } from "../types";

interface EnvVarsModalProps {
  workspace: Workspace;
  onClose: () => void;
  onError: (message: string | null) => void;
}

export default function EnvVarsModal({ workspace, onClose, onError }: EnvVarsModalProps) {
  const [vars, setVars] = useState<EnvVar[]>([]);
  const [loading, setLoading] = useState(true);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getEnvVars(workspace.id)
      .then((list) => !cancelled && setVars(list))
      .catch((e) => !cancelled && onError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id]);

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      void handleSave();
    }
  }

  async function handleSave() {
    const trimmedKey = key.trim();
    if (!trimmedKey || saving) return;
    onError(null);
    setSaving(true);
    try {
      const saved = await setEnvVar(workspace.id, trimmedKey, value);
      setVars((prev) => {
        const existing = prev.findIndex((v) => v.envKey === saved.envKey);
        if (existing >= 0) {
          const copy = [...prev];
          copy[existing] = saved;
          return copy;
        }
        // Keep alphabetical order like the backend query.
        return [...prev, saved].sort((a, b) =>
          a.envKey.localeCompare(b.envKey, undefined, { sensitivity: "base" })
        );
      });
      setKey("");
      setValue("");
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: string) {
    onError(null);
    const snapshot = vars;
    setVars((prev) => prev.filter((v) => v.id !== id));
    try {
      await deleteEnvVar(id);
    } catch (e) {
      onError(String(e));
      setVars(snapshot);
    }
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      onKeyDown={onKeyDown}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
    >
      <div className="mx-4 w-full max-w-md rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl">
        <div className="flex items-center gap-2 border-b border-zinc-800 px-4 py-3">
          <KeyRound size={14} className="text-zinc-500" />
          <h3 className="text-sm font-medium text-zinc-200">
            Environment variables
          </h3>
          <span className="truncate text-[11px] text-zinc-500">
            · {workspace.name}
          </span>
          <button
            onClick={onClose}
            title="Close (Esc)"
            className="ml-auto rounded p-1 text-zinc-500 transition-colors hover:bg-zinc-800 hover:text-zinc-300"
          >
            <X size={14} />
          </button>
        </div>

        <p className="px-4 pt-3 pb-1 text-[11px] leading-relaxed text-zinc-500">
          Injected into every automation script run in this workspace.
        </p>

        <div className="max-h-56 overflow-y-auto px-4 pb-2">
          {loading ? (
            <p className="py-4 text-center text-xs text-zinc-600">Loading…</p>
          ) : vars.length === 0 ? (
            <p className="py-4 text-center text-xs text-zinc-600">
              No variables defined yet.
            </p>
          ) : (
            <ul className="space-y-1">
              {vars.map((v) => (
                <li
                  key={v.id}
                  className="group flex items-center gap-2 rounded bg-zinc-950/70 px-2.5 py-1.5 ring-1 ring-zinc-800"
                >
                  <span className="shrink-0 font-mono text-[11px] text-indigo-300">
                    {v.envKey}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-zinc-400">
                    = {v.envValue || "«empty»"}
                  </span>
                  <button
                    onClick={() => void handleDelete(v.id)}
                    title={`Delete ${v.envKey}`}
                    className="rounded p-0.5 text-zinc-600 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100"
                  >
                    <X size={12} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="border-t border-zinc-800 p-3">
          <div className="flex items-center gap-2">
            <input
              value={key}
              onChange={(e) => setKey(e.target.value)}
              placeholder="NAME"
              maxLength={128}
              spellCheck={false}
              autoFocus
              className="w-28 min-w-0 rounded bg-zinc-800 px-2 py-1.5 font-mono text-xs text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
            />
            <span className="text-zinc-600">=</span>
            <input
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder="value"
              maxLength={4096}
              spellCheck={false}
              className="min-w-0 flex-1 rounded bg-zinc-800 px-2 py-1.5 font-mono text-xs text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
            />
            <button
              onClick={() => void handleSave()}
              disabled={!key.trim() || saving}
              title="Set variable (Enter)"
              className="flex shrink-0 items-center gap-1 rounded bg-indigo-500/90 px-2 py-1.5 text-[11px] font-medium text-white transition-colors enabled:hover:bg-indigo-400 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {saving ? <Loader2 size={11} className="animate-spin" /> : <Plus size={11} />}
              Set
            </button>
          </div>
          <p className="mt-1.5 text-[10px] text-zinc-600">
            Setting an existing name updates its value.
          </p>
        </div>
      </div>
    </div>
  );
}
