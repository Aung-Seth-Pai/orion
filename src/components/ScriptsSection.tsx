import { useEffect, useRef, useState } from "react";
import {
  KeyRound,
  Loader2,
  Pencil,
  Plus,
  ScrollText,
  Trash2,
  X,
} from "lucide-react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  createScript,
  deleteScript,
  executeScript,
  getScripts,
  updateScript,
} from "../api/scripts";
import type { AutomationScript, ScriptType, Workspace } from "../types";
import EnvVarsModal from "./EnvVarsModal";

const TYPE_LABELS: Record<ScriptType, string> = {
  powershell: "PowerShell",
  cmd: "CMD",
  python: "Python",
  node: "Node.js",
  wsl_bash: "WSL Bash",
};

const TYPE_BADGE: Record<ScriptType, string> = {
  powershell: "bg-blue-500/15 text-blue-300",
  cmd: "bg-yellow-500/15 text-yellow-300",
  python: "bg-sky-500/15 text-sky-300",
  node: "bg-green-500/15 text-green-300",
  wsl_bash: "bg-orange-500/15 text-orange-300",
};

const PLACEHOLDERS: Record<ScriptType, string> = {
  powershell: "Get-Process | Select-Object -First 5",
  cmd: "echo hello from cmd",
  python: 'print("hello from python")',
  node: 'console.log("hello from node")',
  wsl_bash: 'echo "hello from wsl"',
};

const SCRIPT_TYPES = Object.keys(TYPE_LABELS) as ScriptType[];

interface ScriptsSectionProps {
  workspace: Workspace;
  onError: (message: string | null) => void;
}

export default function ScriptsSection({ workspace, onError }: ScriptsSectionProps) {
  const [scripts, setScripts] = useState<AutomationScript[]>([]);
  const [loading, setLoading] = useState(true);

  const [composerOpen, setComposerOpen] = useState(false);
  const [editingScript, setEditingScript] = useState<AutomationScript | null>(null);
  const [title, setTitle] = useState("");
  const [scriptType, setScriptType] = useState<ScriptType>("powershell");
  const [content, setContent] = useState("");
  const [saving, setSaving] = useState(false);

  const [runningId, setRunningId] = useState<string | null>(null);
  const [outputs, setOutputs] = useState<Record<string, string>>({});
  const [envVarsOpen, setEnvVarsOpen] = useState(false);

  const titleRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setOutputs({});
    setEditingScript(null);
    // Workspace-specific scripts plus global scripts (workspace_id IS NULL).
    Promise.all([getScripts(workspace.id), getScripts(null)])
      .then(([own, global]) => {
        if (!cancelled) setScripts([...own, ...global]);
      })
      .catch((e) => !cancelled && onError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id]);

  useEffect(() => {
    if (composerOpen) titleRef.current?.focus();
  }, [composerOpen]);

  function resetComposer() {
    setTitle("");
    setScriptType("powershell");
    setContent("");
    setEditingScript(null);
  }

  function closeComposer() {
    setComposerOpen(false);
    resetComposer();
  }

  function openEdit(script: AutomationScript) {
    setEditingScript(script);
    setTitle(script.title);
    setScriptType(script.scriptType);
    setContent(script.scriptContent);
    setComposerOpen(true);
  }

  async function handleSubmit() {
    if (saving || !title.trim() || !content.trim()) return;
    setSaving(true);
    try {
      if (editingScript) {
        const updated = await updateScript(editingScript.id, {
          title,
          scriptType,
          scriptContent: content,
        });
        setScripts((prev) =>
          prev.map((s) => (s.id === updated.id ? updated : s))
        );
      } else {
        const script = await createScript({
          workspaceId: workspace.id,
          title,
          scriptType,
          scriptContent: content,
        });
        setScripts((prev) => [...prev, script]);
      }
      resetComposer();
      setComposerOpen(false);
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void handleSubmit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeComposer();
    }
  }

  async function handleDelete(id: string) {
    const approved = await confirm(
      "Are you sure you want to delete this script? This action cannot be undone.",
      { title: "Delete Script", kind: "warning" }
    );
    if (!approved) return;

    const snapshot = scripts;
    setScripts((prev) => prev.filter((s) => s.id !== id));
    try {
      await deleteScript(id);
    } catch (e) {
      onError(String(e));
      setScripts(snapshot);
    }
  }

  async function handleRun(id: string) {
    onError(null);
    setRunningId(id);
    try {
      const output = await executeScript(id);
      setOutputs((prev) => ({ ...prev, [id]: output }));
    } catch (e) {
      setOutputs((prev) => ({ ...prev, [id]: `ERROR: ${String(e)}` }));
    } finally {
      setRunningId(null);
    }
  }

  return (
    <section>
      {envVarsOpen && (
        <EnvVarsModal
          workspace={workspace}
          onClose={() => setEnvVarsOpen(false)}
          onError={onError}
        />
      )}
      <div className="mb-2 flex items-center gap-2">
        <ScrollText size={13} className="text-zinc-500" />
        <h3 className="text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">
          Automation Scripts
        </h3>
        <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400 tabular-nums">
          {scripts.length}
        </span>
        <button
          onClick={() => setEnvVarsOpen(true)}
          className="ml-auto flex items-center gap-1 rounded bg-zinc-800 px-2 py-0.5 text-[11px] font-medium text-zinc-300 transition-colors hover:bg-zinc-700 hover:text-zinc-100 cursor-default"
        >
          <KeyRound size={11} />
          Variables
        </button>
        <button
          onClick={() => (composerOpen ? closeComposer() : setComposerOpen(true))}
          className="flex items-center gap-1 rounded bg-zinc-800 px-2 py-0.5 text-[11px] font-medium text-zinc-300 transition-colors hover:bg-zinc-700 hover:text-zinc-100 cursor-default"
        >
          {composerOpen ? <X size={11} /> : <Plus size={11} />}
          {composerOpen ? "Cancel" : "New"}
        </button>
      </div>

      {composerOpen && (
        <div className="mb-3 rounded-lg border border-zinc-700/60 bg-zinc-900 p-3">
          <div className="mb-2 flex gap-2">
            <input
              ref={titleRef}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={onKeyDown}
              maxLength={120}
              placeholder="Script title…"
              className="min-w-0 flex-1 rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
            />
            <select
              value={scriptType}
              onChange={(e) => setScriptType(e.target.value as ScriptType)}
              className="rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] text-zinc-200 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
            >
              {SCRIPT_TYPES.map((t) => (
                <option key={t} value={t}>
                  {TYPE_LABELS[t]}
                </option>
              ))}
            </select>
          </div>
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            onKeyDown={onKeyDown}
            rows={6}
            spellCheck={false}
            placeholder={`# ${PLACEHOLDERS[scriptType]}`}
            className="w-full resize-y rounded bg-zinc-950 p-2.5 font-mono text-xs text-zinc-200 placeholder-zinc-600 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
          />
          <div className="mt-2 flex items-center justify-between">
            <span className="text-[10px] text-zinc-600">
              Ctrl+Enter to save · Esc to cancel
            </span>
            <button
              onClick={() => void handleSubmit()}
              disabled={!title.trim() || !content.trim() || saving}
              className="flex items-center gap-1.5 rounded bg-emerald-600 px-3 py-1 text-xs font-medium text-white transition-colors enabled:hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {saving ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Plus size={12} />
              )}
              {editingScript ? "Update Script" : "Add script"}
            </button>
          </div>
        </div>
      )}

      {loading ? (
        <p className="py-4 text-center text-sm text-zinc-600">Loading scripts…</p>
      ) : scripts.length === 0 && !composerOpen ? (
        <div className="flex flex-col items-center gap-1.5 rounded-lg border border-dashed border-zinc-800 py-6 text-center">
          <ScrollText size={20} className="text-zinc-700" />
          <p className="text-xs text-zinc-500">
            No scripts yet. Automate PowerShell, CMD, Python, Node or WSL Bash.
          </p>
        </div>
      ) : (
        <ul className="space-y-2">
          {scripts.map((script) => {
            const running = runningId === script.id;
            return (
              <li
                key={script.id}
                className="group relative rounded-lg border border-zinc-800 bg-zinc-900/70 transition-colors hover:border-zinc-700"
              >
                <div className="flex items-center gap-2 px-3 pt-2.5 pb-2">
                  <span
                    className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${TYPE_BADGE[script.scriptType]}`}
                  >
                    {TYPE_LABELS[script.scriptType]}
                  </span>
                  <span className="truncate text-[13px] font-medium text-zinc-200">
                    {script.title}
                  </span>
                  {script.workspaceId === null && (
                    <span
                      title="Global script"
                      className="rounded bg-zinc-800 px-1.5 py-0.5 text-[9px] tracking-wide text-zinc-400 uppercase"
                    >
                      global
                    </span>
                  )}
                  <span
                    role="button"
                    tabIndex={-1}
                    title="Delete script"
                    onClick={() => void handleDelete(script.id)}
                    className="ml-auto rounded p-1 text-zinc-600 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100"
                  >
                    <Trash2 size={13} />
                  </span>
                  <button
                    onClick={() => openEdit(script)}
                    title="Edit script"
                    className="rounded p-1 text-zinc-500 opacity-0 transition-opacity hover:text-indigo-300 group-hover:opacity-100 cursor-default"
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    onClick={() => void handleRun(script.id)}
                    disabled={running}
                    title="Execute script"
                    className="flex items-center gap-1 rounded bg-emerald-600/90 px-2 py-0.5 text-[11px] font-medium text-white transition-colors enabled:hover:bg-emerald-500 disabled:opacity-50 cursor-default"
                  >
                    {running ? (
                      <Loader2 size={11} className="animate-spin" />
                    ) : (
                      <PlayGlyph />
                    )}
                    Run
                  </button>
                </div>

                {outputs[script.id] !== undefined && (
                  <pre className="mx-3 mb-3 max-h-48 overflow-auto rounded bg-zinc-950 p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-zinc-300 ring-1 ring-zinc-800">
                    {outputs[script.id]}
                  </pre>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function PlayGlyph() {
  return (
    <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}
