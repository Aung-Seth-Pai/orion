import { useEffect, useRef, useState } from "react";
import {
  ExternalLink,
  Folder,
  Globe,
  Hourglass,
  Loader2,
  Plus,
  Rocket,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  createResource,
  deleteResource,
  getResources,
  findResourceByTarget,
  launchResource,
  reorderResources,
  suggestResources,
} from "../api/resources";
import { getWorkspaceTime, resetWorkspaceTime } from "../api/timers";
import { IDLE_TIMER } from "../types";
import type {
  PreferredApp,
  Resource,
  ResourceSuggestion,
  ResourceType,
  TimerController,
  Workspace,
} from "../types";
import ScriptsSection from "./ScriptsSection";
import TasksSection from "./TasksSection";
import TimerWidget from "./TimerWidget";
import { useDragReorder } from "../utils/reorder";
import { platform } from "../utils/platform";
import { displayTarget } from "../utils/paths";
import ResourceSuggestions from "./ResourceSuggestions";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Code, Terminal } from "lucide-react";

const BROWSER_LABELS: Record<PreferredApp, string> = {
  default: "System default",
  safari: "Safari",
  chrome: "Chrome",
  edge: "Edge",
  firefox: "Firefox",
};

/**
 * The browsers this machine has a launcher for.
 *
 * A resource imported from the other platform may name one that is not here.
 * It keeps its stored preference — so it still opens in Safari when the setup
 * goes back to the Mac — and the launcher opens it in the system default
 * meanwhile. The picker only offers what works, so choosing deliberately and
 * inheriting a choice are kept distinct.
 */
function availableBrowsers(): PreferredApp[] {
  return platform().browsers as PreferredApp[];
}

function browserOptions(current: PreferredApp): PreferredApp[] {
  const available = availableBrowsers();
  return available.includes(current) ? available : [current, ...available];
}

/** Safari has no profile switch; the others take one. */
function supportsProfile(app: PreferredApp): boolean {
  return app !== "default" && app !== "safari";
}

/** Accumulated time as HH:MM:SS, with hours growing past two digits. */
function formatTotalTime(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return [h, m, s].map((part) => String(part).padStart(2, "0")).join(":");
}

interface ResourcesPanelProps {
  workspace: Workspace;
  onError: (message: string | null) => void;
  timer: TimerController;
  /** Selects another workspace, used when jumping to an existing match. */
  onJumpToWorkspace: (id: string) => void;
}

export default function ResourcesPanel({
  workspace,
  onError,
  timer,
  onJumpToWorkspace,
}: ResourcesPanelProps) {
  const [resources, setResources] = useState<Resource[]>([]);
  const [loading, setLoading] = useState(true);
  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [totalSeconds, setTotalSeconds] = useState<number | null>(null);
  const [resettingTime, setResettingTime] = useState(false);

  const [composerOpen, setComposerOpen] = useState(false);
  const [type, setType] = useState<ResourceType>("link");
  const [title, setTitle] = useState("");
  const [targetPath, setTargetPath] = useState("");
  const [preferredApp, setPreferredApp] = useState<PreferredApp>("default");
  const [profileName, setProfileName] = useState("");
  const [saving, setSaving] = useState(false);
  const [similar, setSimilar] = useState<ResourceSuggestion[]>([]);
  const [duplicate, setDuplicate] = useState<ResourceSuggestion | null>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const targetRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getResources(workspace.id)
      .then((list) => {
        if (!cancelled) setResources(list);
      })
      .catch((e) => !cancelled && onError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id]);

  // Refetched on workspace switch and once each finished session is committed
  // (logVersion), so the header total stays in step with the timer widget.
  useEffect(() => {
    let cancelled = false;
    getWorkspaceTime(workspace.id)
      .then((seconds) => {
        if (!cancelled) setTotalSeconds(seconds);
      })
      .catch((e) => !cancelled && onError(String(e)));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id, timer.logVersion]);

  useEffect(() => {
    if (composerOpen) titleRef.current?.focus();
  }, [composerOpen]);

  // Debounced so a lookup runs once the typing pauses rather than per keystroke.
  // Both effects clear their own results when the composer closes or the field
  // empties, so a stale hint never outlives what produced it.
  useEffect(() => {
    if (!composerOpen || title.trim().length < 2) {
      setSimilar([]);
      return;
    }
    let cancelled = false;
    const handle = window.setTimeout(() => {
      suggestResources(title)
        .then((found) => {
          if (!cancelled) setSimilar(found);
        })
        // A suggestion box is a convenience; a failure here must not interrupt
        // the user mid-entry with an error banner.
        .catch(() => {});
    }, 220);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [composerOpen, title]);

  useEffect(() => {
    if (!composerOpen || !targetPath.trim()) {
      setDuplicate(null);
      return;
    }
    let cancelled = false;
    const handle = window.setTimeout(() => {
      findResourceByTarget(type, targetPath)
        .then((found) => {
          if (!cancelled) setDuplicate(found);
        })
        .catch(() => {});
    }, 220);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [composerOpen, targetPath, type]);

  const {
    draggingId,
    overId,
    itemProps: dragProps,
  } = useDragReorder(resources, handleReorder);

  async function handleResetTime() {
    if (resettingTime) return;
    const approved = await confirm(
      "Reset accumulated time for this workspace?",
      { title: "Reset Total Time", kind: "warning" }
    );
    if (!approved) return;

    setResettingTime(true);
    try {
      await resetWorkspaceTime(workspace.id);
      setTotalSeconds(0);
      // So the sidebar's mark dims with it, rather than keeping the brightness
      // of time that has just been cleared.
      timer.markChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setResettingTime(false);
    }
  }

  function resetComposer() {
    setType("link");
    setTitle("");
    setTargetPath("");
    setPreferredApp("default");
    setProfileName("");
    setSimilar([]);
    setDuplicate(null);
  }

  async function handleCreate() {
    if (saving) return;
    if (!title.trim() || !targetPath.trim()) {
      titleRef.current?.focus();
      return;
    }
    setSaving(true);
    try {
      const resource = await createResource({
        workspaceId: workspace.id,
        type,
        title,
        targetPath,
        preferredApp: type === "link" ? preferredApp : "default",
        profileName:
          type === "link" && supportsProfile(preferredApp) && profileName.trim()
            ? profileName
            : null,
      });
      setResources((prev) => [...prev, resource]);
      resetComposer();
      setComposerOpen(false);
      targetRef.current?.focus();
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: string) {
    const snapshot = resources;
    setResources((prev) => prev.filter((r) => r.id !== id));
    try {
      await deleteResource(id);
    } catch (e) {
      onError(String(e));
      setResources(snapshot);
    }
  }

  function handleReorder(ordered: Resource[]) {
    onError(null);
    const snapshot = resources;
    setResources(ordered);
    reorderResources(
      workspace.id,
      ordered.map((r) => r.id)
    ).catch((e) => {
      onError(String(e));
      setResources(snapshot);
    });
  }

  /// Copies a resource's target. The displayed form is copied rather than the
  /// stored one, so a local file yields the path the user typed instead of the
  /// file:// URL it is stored as — the path is what pastes usefully into
  /// the file manager, a terminal or a chat.
  async function handleCopy(res: Resource) {
    onError(null);
    try {
      await writeText(displayTarget(res.targetPath));
      setCopiedId(res.id);
      window.setTimeout(
        () => setCopiedId((current) => (current === res.id ? null : current)),
        1400
      );
    } catch (e) {
      onError(`Could not copy to the clipboard: ${String(e)}`);
    }
  }

  async function handleLaunch(id: string, actionOverride?: "ide" | "terminal") {
    onError(null);
    setLaunchingId(id);
    try {
      await launchResource(id, actionOverride);
    } catch (e) {
      onError(String(e));
    } finally {
      setLaunchingId(null);
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void handleCreate();
    } else if (e.key === "Escape") {
      e.preventDefault();
      setComposerOpen(false);
      resetComposer();
    }
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2.5 border-b border-rule/80 px-4">
        <span
          aria-hidden
          className={`size-2 rounded-full ${workspace.color ? "" : "bg-ink-ghost"}`}
          style={workspace.color ? { backgroundColor: workspace.color } : undefined}
        />
        <h2 className="truncate text-sm font-medium text-ink">{workspace.name}</h2>

        <div
          title="Total time tracked in this workspace"
          className="flex shrink-0 items-center gap-1.5 rounded-md bg-raised/70 px-2 py-1 ring-1 ring-rule-strong/50"
        >
          <Hourglass size={11} className="text-ink-faint" />
          <span className="font-mono text-[11px] tabular-nums text-ink-muted">
            {totalSeconds === null ? "--:--:--" : formatTotalTime(totalSeconds)}
          </span>
          <button
            onClick={() => void handleResetTime()}
            disabled={totalSeconds === null || resettingTime}
            title="Reset accumulated time"
            className="rounded p-0.5 text-ink-faint transition-colors enabled:hover:bg-raised-hover/60 enabled:hover:text-ink-muted disabled:opacity-40 cursor-default"
          >
            <RotateCcw size={11} className={resettingTime ? "animate-spin" : ""} />
          </button>
        </div>

        <div className="ml-auto flex items-center gap-2">
          {/* No key prop: the widget is controlled by global timer state in
              App, so sessions keep running across workspace switches. */}
          <TimerWidget
            timer={timer.states[workspace.id] ?? IDLE_TIMER}
            pomodoroSeconds={timer.pomodoroSeconds}
            onStart={(mode) => timer.start(workspace.id, mode)}
            onStop={() => timer.stop(workspace.id)}
            onReset={() => timer.reset(workspace.id)}
          />
          <button
            onClick={() => setComposerOpen((v) => !v)}
            className="flex items-center gap-1.5 rounded bg-accent/90 px-2.5 py-1 text-xs font-medium text-accent-on transition-colors hover:bg-accent-bright cursor-default"
          >
            {composerOpen ? <X size={13} /> : <Plus size={13} />}
            {composerOpen ? "Close" : "Add resource"}
          </button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {composerOpen && (
          <div className="mb-4 rounded-lg border border-rule-strong/60 bg-surface p-3">
            <div className="mb-3 inline-flex overflow-hidden rounded-md ring-1 ring-rule-strong">
              {(["link", "folder"] as ResourceType[]).map((t) => (
                <button
                  key={t}
                  onClick={() => setType(t)}
                  className={`flex items-center gap-1.5 px-3 py-1 text-xs font-medium transition-colors cursor-default ${
                    type === t
                      ? "bg-accent text-accent-on"
                      : "text-ink-muted hover:text-ink"
                  }`}
                >
                  {t === "link" ? <Globe size={12} /> : <Folder size={12} />}
                  {t === "link" ? "Link" : "Folder"}
                </button>
              ))}
            </div>

            <div className="grid grid-cols-2 gap-2">
              <input
                ref={titleRef}
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                onKeyDown={onKeyDown}
                maxLength={120}
                placeholder="Title"
                className="w-full rounded bg-raised px-2.5 py-1.5 text-[13px] text-ink-strong placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50"
              />
              <input
                ref={targetRef}
                value={targetPath}
                onChange={(e) => setTargetPath(e.target.value)}
                onKeyDown={onKeyDown}
                maxLength={2048}
                placeholder={
                  type === "link"
                    ? "https://example.com  or  C:\\Users\\…\\file.pdf"
                    : "C:\\Users\\…\\Projects"
                }
                className="col-span-2 w-full rounded bg-raised px-2.5 py-1.5 text-[13px] text-ink-strong placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50"
              />
              {type === "link" && (
                <p className="col-span-2 -mt-1 text-[10px] text-ink-faint">
                  A web URL, or an absolute path to a local file —{" "}
                  {platform().os === "macos"
                    ? "Finder’s “Copy as Pathname” (⌥⌘C)"
                    : "Explorer’s “Copy as path”"}{" "}
                  works as-is.
                </p>
              )}

              <ResourceSuggestions
                duplicate={duplicate}
                similar={similar}
                onOpen={(suggestion) => {
                  // Close the composer and jump to where the match already
                  // lives, which is the whole point of surfacing it.
                  setComposerOpen(false);
                  resetComposer();
                  onJumpToWorkspace(suggestion.workspaceId);
                }}
              />

              {type === "link" && (
                <>
                  <select
                    value={preferredApp}
                    onChange={(e) => setPreferredApp(e.target.value as PreferredApp)}
                    className="w-full rounded bg-raised px-2.5 py-1.5 text-[13px] text-ink outline-none ring-1 ring-transparent focus:ring-focus/50"
                  >
                    {browserOptions(preferredApp).map((app) => (
                      <option key={app} value={app}>
                        {BROWSER_LABELS[app]}
                        {app === "default" || availableBrowsers().includes(app)
                          ? ""
                          : " — not on this platform"}
                      </option>
                    ))}
                  </select>
                  <input
                    value={profileName}
                    onChange={(e) => setProfileName(e.target.value)}
                    onKeyDown={onKeyDown}
                    maxLength={120}
                    disabled={!supportsProfile(preferredApp)}
                    placeholder={
                      preferredApp === "firefox"
                        ? "Profile name (optional)"
                        : "Profile dir e.g. Profile 1"
                    }
                    className={`w-full rounded bg-raised px-2.5 py-1.5 text-[13px] placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50 ${
                      supportsProfile(preferredApp)
                        ? "text-ink-strong"
                        : "cursor-not-allowed opacity-40"
                    }`}
                  />
                </>
              )}
            </div>

            <div className="mt-3 flex justify-end">
              <button
                onClick={() => void handleCreate()}
                disabled={!title.trim() || !targetPath.trim() || saving}
                className="flex items-center gap-1.5 rounded bg-positive px-3 py-1 text-xs font-medium text-accent-on transition-colors enabled:hover:bg-positive-ink disabled:cursor-not-allowed disabled:opacity-40"
              >
                {saving ? <Loader2 size={12} className="animate-spin" /> : <Plus size={12} />}
                Add
              </button>
            </div>
          </div>
        )}

        {loading ? (
          <p className="pt-8 text-center text-sm text-ink-faint">Loading resources…</p>
        ) : resources.length === 0 ? (
          <div className="flex flex-col items-center gap-2 pt-10 pb-8 text-center">
            <ExternalLink size={26} className="text-ink-ghost" />
            <p className="text-sm text-ink-faint">No resources in this workspace</p>
            <p className="max-w-xs text-xs leading-relaxed text-ink-faint">
              Add links or folder paths above to launch them from here.
            </p>
          </div>
        ) : (
          <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {resources.map((res) => (
              <li
                key={res.id}
                {...dragProps(res.id)}
                className={`group relative flex flex-col rounded-lg border bg-surface/70 transition-colors ${
                  draggingId === res.id ? "opacity-40" : ""
                } ${
                  overId === res.id
                    ? "border-accent/70"
                    : "border-rule hover:border-rule-strong"
                }`}
              >
                <span
                  role="button"
                  tabIndex={-1}
                  title="Delete resource"
                  onClick={() => void handleDelete(res.id)}
                  className="absolute top-1.5 right-1.5 rounded p-1 text-ink-faint opacity-0 transition-opacity hover:text-danger-ink group-hover:opacity-100"
                >
                  <Trash2 size={13} />
                </span>

                {/* The card body copies; the launch buttons below are separate
                    so a click never has to choose between the two. */}
                <div
                  onClick={() => void handleCopy(res)}
                  title="Click to copy"
                  className="flex min-w-0 cursor-default items-start gap-2.5 p-3 pr-8"
                >
                  <span
                    className={`mt-0.5 flex size-7 shrink-0 items-center justify-center rounded ${
                      res.type === "link"
                        ? "bg-accent/15 text-accent-ink"
                        : "bg-positive/15 text-positive-ink"
                    }`}
                  >
                    {res.type === "link" ? <Globe size={14} /> : <Folder size={14} />}
                  </span>
                  <div className="min-w-0">
                    <p className="truncate text-[13px] font-medium text-ink">
                      {res.title}
                    </p>
                    <p className="truncate text-[11px] text-ink-faint">
                      {copiedId === res.id ? (
                        <span className="text-positive-ink">Copied to clipboard</span>
                      ) : (
                        displayTarget(res.targetPath)
                      )}
                    </p>
                    {res.type === "link" && res.preferredApp !== "default" && (
                      <p className="mt-1 inline-block rounded bg-raised px-1.5 py-0.5 text-[10px] text-ink-muted">
                        {BROWSER_LABELS[res.preferredApp]}
                        {res.profileName ? ` · ${res.profileName}` : ""}
                      </p>
                    )}
                  </div>
                </div>

                {res.type === "folder" ? (
                  <div className="m-2 mt-0 flex items-center gap-1.5">
                    <button
                      onClick={() => void handleLaunch(res.id, "ide")}
                      disabled={launchingId === res.id}
                      title="Open in custom IDE"
                      className="flex size-7 shrink-0 items-center justify-center rounded border border-rule-strong text-ink-muted transition-colors enabled:hover:border-info-ink/60 enabled:hover:text-info-ink disabled:opacity-50 cursor-default"
                    >
                      <Code size={13} />
                    </button>
                    <button
                      onClick={() => void handleLaunch(res.id, "terminal")}
                      disabled={launchingId === res.id}
                      title={`Open in ${platform().terminalLabel}`}
                      className="flex size-7 shrink-0 items-center justify-center rounded border border-rule-strong text-ink-muted transition-colors enabled:hover:border-positive/60 enabled:hover:text-positive-ink disabled:opacity-50 cursor-default"
                    >
                      <Terminal size={13} />
                    </button>
                    <button
                      onClick={() => void handleLaunch(res.id)}
                      disabled={launchingId === res.id}
                      className="flex flex-1 items-center justify-center gap-1.5 rounded border border-accent/30 bg-accent/10 px-2 py-1.5 text-xs font-medium text-accent-ink transition-colors enabled:hover:border-accent/60 enabled:hover:bg-accent/20 enabled:hover:text-accent-bright disabled:opacity-50 cursor-default"
                    >
                      {launchingId === res.id ? (
                        <Loader2 size={12} className="animate-spin" />
                      ) : (
                        <Rocket size={12} />
                      )}
                      Open
                    </button>
                  </div>
                ) : (
                  <button
                    onClick={() => void handleLaunch(res.id)}
                    disabled={launchingId === res.id}
                    className="m-2 mt-0 flex items-center justify-center gap-1.5 rounded border border-accent/30 bg-accent/10 px-2 py-1.5 text-xs font-medium text-accent-ink transition-colors enabled:hover:border-accent/60 enabled:hover:bg-accent/20 enabled:hover:text-accent-bright disabled:opacity-50 cursor-default"
                  >
                    {launchingId === res.id ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Rocket size={12} />
                    )}
                    Launch
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}

        <div className="my-5 border-t border-rule/80" />

        <TasksSection workspace={workspace} onError={onError} />

        <div className="my-5 border-t border-rule/80" />

        <ScriptsSection workspace={workspace} onError={onError} />
      </div>
    </section>
  );
}
