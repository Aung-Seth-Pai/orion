import { useEffect, useMemo, useRef, useState } from "react";
import { Loader2, Plus, Rocket, Settings as SettingsIcon, Trash2, X } from "lucide-react";
import type { Workspace } from "../types";
import { useAppVersion } from "../utils/version";
import { useDragReorder } from "../utils/reorder";
import { depthFromHours } from "../utils/constellation";
import ConstellationMark from "./ConstellationMark";

// User-chosen workspace colours. Deliberately NOT tokens: these are data the
// user picked, and they must look the same whichever theme is active.
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

/**
 * Fixed rather than read from the --mark-size token: the size drives the
 * optical stroke weights inside the figure, so it has to be a real number at
 * render time, and pulling it out of CSS would mean a getComputedStyle on
 * every row and a re-measure on every theme change.
 */
const MARK_SIZE = 20;

interface SidebarProps {
  workspaces: Workspace[];
  /** Accumulated seconds per workspace id; absent ids read as untouched. */
  workspaceTimes: Record<string, number>;
  activeId: string | null;
  settingsActive: boolean;
  onSelect: (id: string) => void;
  onOpenSettings: () => void;
  onCreate: (name: string, color: string | null) => Promise<void>;
  onDelete: (id: string) => void;
  onReorder: (ordered: Workspace[]) => void;
}

export default function Sidebar({
  workspaces,
  workspaceTimes,
  activeId,
  settingsActive,
  onSelect,
  onOpenSettings,
  onCreate,
  onDelete,
  onReorder,
}: SidebarProps) {
  const appVersion = useAppVersion();
  const { draggingId, overId, itemProps } = useDragReorder(workspaces, onReorder);

  // The ceiling for every mark is the user's own busiest workspace, so it is
  // computed once per render rather than per row -- and it is what keeps the
  // marks meaningful whether the user has three hours logged or three thousand.
  const maxHours = useMemo(() => {
    const totals = workspaces.map((w) => (workspaceTimes[w.id] ?? 0) / 3600);
    return totals.length ? Math.max(...totals) : 0;
  }, [workspaces, workspaceTimes]);
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
    <aside className="flex w-60 shrink-0 flex-col border-r border-rule/80 bg-surface/40">
      <div className="flex h-12 items-center gap-2.5 px-4">
        <Rocket size={16} className="text-accent-bright" strokeWidth={2.2} />
        <span className="text-[13px] font-semibold tracking-widest text-ink-strong uppercase">
          Orion
        </span>
        <span className="ml-auto rounded bg-raised px-1.5 py-0.5 text-[10px] font-medium text-ink-muted tabular-nums">
          {workspaces.length}
        </span>
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-2 pb-2">
        {workspaces.length > 0 && (
          <p className="px-2 pt-1 pb-1.5 text-[10px] font-semibold tracking-wider text-ink-faint uppercase">
            Workspaces
          </p>
        )}
        <ul className="space-y-0.5">
          {workspaces.map((ws) => {
            const active = ws.id === activeId;
            return (
              <li
                key={ws.id}
                {...itemProps(ws.id)}
                className={`rounded-md transition-opacity ${
                  draggingId === ws.id ? "opacity-40" : ""
                } ${
                  overId === ws.id ? "ring-1 ring-accent/60" : ""
                }`}
              >
                <button
                  onClick={() => onSelect(ws.id)}
                  title="Drag to reorder"
                  className={`group flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors duration-100 cursor-default ${
                    active
                      ? "bg-raised-hover/50 text-ink-strong"
                      : "text-ink-muted hover:bg-raised/50 hover:text-ink"
                  }`}
                >
                  {/* The workspace's own colour still carries it -- the mark
                      is drawn in currentColor, so tinting the wrapper keeps
                      that choice meaningful while the shape adds an identity
                      the colour alone could not. With no colour set it
                      inherits the row's active/hover state instead. */}
                  <span
                    className="shrink-0"
                    style={ws.color ? { color: ws.color } : undefined}
                  >
                    <ConstellationMark
                      seed={ws.id}
                      depth={depthFromHours(
                        (workspaceTimes[ws.id] ?? 0) / 3600,
                        maxHours
                      )}
                      size={MARK_SIZE}
                    />
                  </span>
                  <span className="truncate">{ws.name}</span>
                  <span
                    role="button"
                    tabIndex={-1}
                    title="Delete workspace"
                    onClick={(e) => {
                      e.stopPropagation();
                      onDelete(ws.id);
                    }}
                    className="ml-auto rounded p-0.5 text-ink-faint opacity-0 transition-opacity hover:text-danger-ink group-hover:opacity-100 focus:opacity-100"
                  >
                    <Trash2 size={13} />
                  </span>
                </button>
              </li>
            );
          })}
        </ul>

        {workspaces.length === 0 && !composerOpen && (
          <p className="px-2 pt-4 pb-2 text-xs leading-relaxed text-ink-faint">
            No workspaces yet.
            <br />
            Create your first one below.
          </p>
        )}
      </div>

      <div className="border-t border-rule/80 p-2">
        {composerOpen ? (
          <div className="rounded-md bg-raised/70 p-2 ring-1 ring-accent/30">
            <div className="flex items-center gap-1.5">
              <input
                ref={inputRef}
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={onKeyDown}
                maxLength={100}
                placeholder="Workspace name…"
                className="min-w-0 flex-1 bg-transparent text-[13px] text-ink-strong placeholder-ink-faint outline-none"
              />
              <button
                onClick={closeComposer}
                title="Cancel (Esc)"
                className="rounded p-0.5 text-ink-faint hover:text-ink-muted"
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
                      ? "scale-110 ring-2 ring-ink-strong/70"
                      : "hover:scale-110"
                  }`}
                  style={{ backgroundColor: c }}
                />
              ))}
              <button
                onClick={() => void submit()}
                disabled={!name.trim() || creating}
                title="Create (Enter)"
                className="ml-auto flex items-center rounded bg-accent px-2 py-0.5 text-[11px] font-medium text-accent-on transition-colors enabled:hover:bg-accent-bright disabled:cursor-not-allowed disabled:opacity-40"
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
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[13px] text-ink-muted transition-colors hover:bg-raised/50 hover:text-ink cursor-default"
          >
            <Plus size={15} />
            New workspace
          </button>
        )}

        <div className="mt-2 flex items-center justify-between border-t border-rule/80 pt-2">
          <button
            onClick={onOpenSettings}
            title="Settings"
            className={`rounded p-1.5 transition-colors ${
              settingsActive
                ? "bg-accent/15 text-accent-bright"
                : "text-ink-faint hover:bg-raised/50 hover:text-ink-muted"
            }`}
          >
            <SettingsIcon size={14} />
          </button>
          <span className="pr-1 text-[10px] text-ink-ghost">
            {appVersion ? `v${appVersion}` : ""}
          </span>
        </div>
      </div>
    </aside>
  );
}
