import { useEffect, useRef, useState } from "react";
import { Loader2, Plus, Rocket, Settings as SettingsIcon, Trash2, X } from "lucide-react";
import type { Workspace } from "../types";
import { useAppVersion } from "../utils/version";

const PALETTE = [
  "#ef4444",
  "#f97316",
  "#eab308",
  "#22c55e",
  "#06b6d4",
  "#3b82f6",
  "#8b5cf6",
  "#ec4899",
];

interface SidebarProps {
  workspaces: Workspace[];
  activeId: string | null;
  settingsActive: boolean;
  onSelect: (id: string) => void;
  onOpenSettings: () => void;
  onCreate: (name: string, color: string | null) => Promise<void>;
  onDelete: (id: string) => void;
}

export default function Sidebar({
  workspaces,
  activeId,
  settingsActive,
  onSelect,
  onOpenSettings,
  onCreate,
  onDelete,
}: SidebarProps) {
  const appVersion = useAppVersion();
  const [composerOpen, setComposerOpen] = useState(false);
  const [name, setName] = useState("");
  const [color, setColor] = useState<string>(PALETTE[5]);
  const [creating, setCreating] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (composerOpen) inputRef.current?.focus();
  }, [composerOpen]);

  function closeComposer() {
    setComposerOpen(false);
    setName("");
    setColor(PALETTE[5]);
  }

  async function submit() {
    const trimmed = name.trim();
    if (!trimmed || creating) return;
    setCreating(true);
    try {
      await onCreate(trimmed, color);
      closeComposer();
    } finally {
      setCreating(false);
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeComposer();
    }
  }

  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-zinc-800/80 bg-zinc-900/40">
      <div className="flex h-12 items-center gap-2.5 px-4">
        <Rocket size={16} className="text-indigo-400" strokeWidth={2.2} />
        <span className="text-[13px] font-semibold tracking-widest text-zinc-100 uppercase">
          Orion
        </span>
        <span className="ml-auto rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] font-medium text-zinc-400 tabular-nums">
          {workspaces.length}
        </span>
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-2 pb-2">
        {workspaces.length > 0 && (
          <p className="px-2 pt-1 pb-1.5 text-[10px] font-semibold tracking-wider text-zinc-500 uppercase">
            Workspaces
          </p>
        )}
        <ul className="space-y-0.5">
          {workspaces.map((ws) => {
            const active = ws.id === activeId;
            return (
              <li key={ws.id}>
                <button
                  onClick={() => onSelect(ws.id)}
                  className={`group flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors duration-100 cursor-default ${
                    active
                      ? "bg-zinc-700/50 text-zinc-100"
                      : "text-zinc-400 hover:bg-zinc-800/50 hover:text-zinc-200"
                  }`}
                >
                  <span
                    aria-hidden
                    className={`size-2 shrink-0 rounded-full ${ws.color ? "" : "bg-zinc-600"}`}
                    style={ws.color ? { backgroundColor: ws.color } : undefined}
                  />
                  <span className="truncate">{ws.name}</span>
                  <span
                    role="button"
                    tabIndex={-1}
                    title="Delete workspace"
                    onClick={(e) => {
                      e.stopPropagation();
                      onDelete(ws.id);
                    }}
                    className="ml-auto rounded p-0.5 text-zinc-500 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100 focus:opacity-100"
                  >
                    <Trash2 size={13} />
                  </span>
                </button>
              </li>
            );
          })}
        </ul>

        {workspaces.length === 0 && !composerOpen && (
          <p className="px-2 pt-4 pb-2 text-xs leading-relaxed text-zinc-600">
            No workspaces yet.
            <br />
            Create your first one below.
          </p>
        )}
      </div>

      <div className="border-t border-zinc-800/80 p-2">
        {composerOpen ? (
          <div className="rounded-md bg-zinc-800/70 p-2 ring-1 ring-indigo-500/30">
            <div className="flex items-center gap-1.5">
              <input
                ref={inputRef}
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={onKeyDown}
                maxLength={100}
                placeholder="Workspace name…"
                className="min-w-0 flex-1 bg-transparent text-[13px] text-zinc-100 placeholder-zinc-500 outline-none"
              />
              <button
                onClick={closeComposer}
                title="Cancel (Esc)"
                className="rounded p-0.5 text-zinc-500 hover:text-zinc-300"
              >
                <X size={14} />
              </button>
            </div>
            <div className="mt-2 flex items-center gap-1">
              {PALETTE.map((c) => (
                <button
                  key={c}
                  onClick={() => setColor(c)}
                  title={`Color ${c}`}
                  className={`size-3.5 rounded-full transition-transform ${
                    color === c
                      ? "scale-110 ring-2 ring-white/70"
                      : "hover:scale-110"
                  }`}
                  style={{ backgroundColor: c }}
                />
              ))}
              <button
                onClick={() => void submit()}
                disabled={!name.trim() || creating}
                title="Create (Enter)"
                className="ml-auto flex items-center rounded bg-indigo-500 px-2 py-0.5 text-[11px] font-medium text-white transition-colors enabled:hover:bg-indigo-400 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {creating ? (
                  <Loader2 size={12} className="animate-spin" />
                ) : (
                  "Add"
                )}
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setComposerOpen(true)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[13px] text-zinc-400 transition-colors hover:bg-zinc-800/50 hover:text-zinc-200 cursor-default"
          >
            <Plus size={15} />
            New workspace
          </button>
        )}

        <div className="mt-2 flex items-center justify-between border-t border-zinc-800/80 pt-2">
          <button
            onClick={onOpenSettings}
            title="Settings"
            className={`rounded p-1.5 transition-colors ${
              settingsActive
                ? "bg-indigo-500/15 text-indigo-400"
                : "text-zinc-500 hover:bg-zinc-800/50 hover:text-zinc-300"
            }`}
          >
            <SettingsIcon size={14} />
          </button>
          <span className="pr-1 text-[10px] text-zinc-700">
            {appVersion ? `v${appVersion}` : ""}
          </span>
        </div>
      </div>
    </aside>
  );
}
