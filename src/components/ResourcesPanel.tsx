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
import { createResource, deleteResource, getResources, launchResource } from "../api/resources";
import { getWorkspaceTime, resetWorkspaceTime } from "../api/timers";
import { IDLE_TIMER } from "../types";
import type {
  PreferredApp,
  Resource,
  ResourceType,
  TimerController,
  Workspace,
} from "../types";
import ScriptsSection from "./ScriptsSection";
import TasksSection from "./TasksSection";
import TimerWidget from "./TimerWidget";
import { Code, Terminal } from "lucide-react";

const BROWSER_LABELS: Record<PreferredApp, string> = {
  default: "System default",
  chrome: "Chrome",
  edge: "Edge",
  firefox: "Firefox",
};

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
}

export default function ResourcesPanel({
  workspace,
  onError,
  timer,
}: ResourcesPanelProps) {
  const [resources, setResources] = useState<Resource[]>([]);
  const [loading, setLoading] = useState(true);
  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [totalSeconds, setTotalSeconds] = useState<number | null>(null);
  const [resettingTime, setResettingTime] = useState(false);

  const [composerOpen, setComposerOpen] = useState(false);
  const [type, setType] = useState<ResourceType>("link");
  const [title, setTitle] = useState("");
  const [targetPath, setTargetPath] = useState("");
  const [preferredApp, setPreferredApp] = useState<PreferredApp>("default");
  const [profileName, setProfileName] = useState("");
  const [saving, setSaving] = useState(false);
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
          type === "link" && preferredApp !== "default" && profileName.trim()
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
      <header className="flex h-12 shrink-0 items-center gap-2.5 border-b border-zinc-800/80 px-4">
        <span
          aria-hidden
          className={`size-2 rounded-full ${workspace.color ? "" : "bg-zinc-600"}`}
          style={workspace.color ? { backgroundColor: workspace.color } : undefined}
        />
        <h2 className="truncate text-sm font-medium text-zinc-200">{workspace.name}</h2>

        <div
          title="Total time tracked in this workspace"
          className="flex shrink-0 items-center gap-1.5 rounded-md bg-zinc-800/70 px-2 py-1 ring-1 ring-zinc-700/50"
        >
          <Hourglass size={11} className="text-zinc-500" />
          <span className="font-mono text-[11px] tabular-nums text-zinc-300">
            {totalSeconds === null ? "--:--:--" : formatTotalTime(totalSeconds)}
          </span>
          <button
            onClick={() => void handleResetTime()}
            disabled={totalSeconds === null || resettingTime}
            title="Reset accumulated time"
            className="rounded p-0.5 text-zinc-500 transition-colors enabled:hover:bg-zinc-700/60 enabled:hover:text-zinc-300 disabled:opacity-40 cursor-default"
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
            className="flex items-center gap-1.5 rounded bg-indigo-500/90 px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-indigo-400 cursor-default"
          >
            {composerOpen ? <X size={13} /> : <Plus size={13} />}
            {composerOpen ? "Close" : "Add resource"}
          </button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {composerOpen && (
          <div className="mb-4 rounded-lg border border-zinc-700/60 bg-zinc-900 p-3">
            <div className="mb-3 inline-flex overflow-hidden rounded-md ring-1 ring-zinc-700">
              {(["link", "folder"] as ResourceType[]).map((t) => (
                <button
                  key={t}
                  onClick={() => setType(t)}
                  className={`flex items-center gap-1.5 px-3 py-1 text-xs font-medium transition-colors cursor-default ${
                    type === t
                      ? "bg-indigo-500 text-white"
                      : "text-zinc-400 hover:text-zinc-200"
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
                className="w-full rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
              />
              <input
                ref={targetRef}
                value={targetPath}
                onChange={(e) => setTargetPath(e.target.value)}
                onKeyDown={onKeyDown}
                maxLength={2048}
                placeholder={
                  type === "link" ? "https://example.com" : "C:\\Users\\…\\Projects"
                }
                className="col-span-2 w-full rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] text-zinc-100 placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
              />
              {type === "link" && (
                <p className="col-span-2 -mt-1 text-[10px] text-zinc-600">
                  Must start with http://, https://, or file://
                </p>
              )}

              {type === "link" && (
                <>
                  <select
                    value={preferredApp}
                    onChange={(e) => setPreferredApp(e.target.value as PreferredApp)}
                    className="w-full rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] text-zinc-200 outline-none ring-1 ring-transparent focus:ring-indigo-500/50"
                  >
                    {(Object.keys(BROWSER_LABELS) as PreferredApp[]).map((app) => (
                      <option key={app} value={app}>
                        {BROWSER_LABELS[app]}
                      </option>
                    ))}
                  </select>
                  <input
                    value={profileName}
                    onChange={(e) => setProfileName(e.target.value)}
                    onKeyDown={onKeyDown}
                    maxLength={120}
                    disabled={preferredApp === "default"}
                    placeholder={
                      preferredApp === "firefox"
                        ? "Profile name (optional)"
                        : "Profile dir e.g. Profile 1"
                    }
                    className={`w-full rounded bg-zinc-800 px-2.5 py-1.5 text-[13px] placeholder-zinc-500 outline-none ring-1 ring-transparent focus:ring-indigo-500/50 ${
                      preferredApp === "default"
                        ? "cursor-not-allowed opacity-40"
                        : "text-zinc-100"
                    }`}
                  />
                </>
              )}
            </div>

            <div className="mt-3 flex justify-end">
              <button
                onClick={() => void handleCreate()}
                disabled={!title.trim() || !targetPath.trim() || saving}
                className="flex items-center gap-1.5 rounded bg-emerald-600 px-3 py-1 text-xs font-medium text-white transition-colors enabled:hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {saving ? <Loader2 size={12} className="animate-spin" /> : <Plus size={12} />}
                Add
              </button>
            </div>
          </div>
        )}

        {loading ? (
          <p className="pt-8 text-center text-sm text-zinc-600">Loading resources…</p>
        ) : resources.length === 0 ? (
          <div className="flex flex-col items-center gap-2 pt-10 pb-8 text-center">
            <ExternalLink size={26} className="text-zinc-700" />
            <p className="text-sm text-zinc-500">No resources in this workspace</p>
            <p className="max-w-xs text-xs leading-relaxed text-zinc-600">
              Add links or folder paths above to launch them from here.
            </p>
          </div>
        ) : (
          <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {resources.map((res) => (
              <li
                key={res.id}
                className="group relative flex flex-col rounded-lg border border-zinc-800 bg-zinc-900/70 transition-colors hover:border-zinc-700"
              >
                <span
                  role="button"
                  tabIndex={-1}
                  title="Delete resource"
                  onClick={() => void handleDelete(res.id)}
                  className="absolute top-1.5 right-1.5 rounded p-1 text-zinc-600 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100"
                >
                  <Trash2 size={13} />
                </span>

                <div className="flex min-w-0 items-start gap-2.5 p-3 pr-8">
                  <span
                    className={`mt-0.5 flex size-7 shrink-0 items-center justify-center rounded ${
                      res.type === "link"
                        ? "bg-indigo-500/15 text-indigo-400"
                        : "bg-emerald-500/15 text-emerald-400"
                    }`}
                  >
                    {res.type === "link" ? <Globe size={14} /> : <Folder size={14} />}
                  </span>
                  <div className="min-w-0">
                    <p className="truncate text-[13px] font-medium text-zinc-200">
                      {res.title}
                    </p>
                    <p className="truncate text-[11px] text-zinc-500">{res.targetPath}</p>
                    {res.type === "link" && res.preferredApp !== "default" && (
                      <p className="mt-1 inline-block rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
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
                      className="flex size-7 shrink-0 items-center justify-center rounded border border-zinc-700 text-zinc-400 transition-colors enabled:hover:border-blue-500/60 enabled:hover:text-blue-300 disabled:opacity-50 cursor-default"
                    >
                      <Code size={13} />
                    </button>
                    <button
                      onClick={() => void handleLaunch(res.id, "terminal")}
                      disabled={launchingId === res.id}
                      title="Open in Windows Terminal"
                      className="flex size-7 shrink-0 items-center justify-center rounded border border-zinc-700 text-zinc-400 transition-colors enabled:hover:border-emerald-500/60 enabled:hover:text-emerald-300 disabled:opacity-50 cursor-default"
                    >
                      <Terminal size={13} />
                    </button>
                    <button
                      onClick={() => void handleLaunch(res.id)}
                      disabled={launchingId === res.id}
                      className="flex flex-1 items-center justify-center gap-1.5 rounded border border-indigo-500/30 bg-indigo-500/10 px-2 py-1.5 text-xs font-medium text-indigo-300 transition-colors enabled:hover:border-indigo-500/60 enabled:hover:bg-indigo-500/20 enabled:hover:text-indigo-200 disabled:opacity-50 cursor-default"
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
                    className="m-2 mt-0 flex items-center justify-center gap-1.5 rounded border border-indigo-500/30 bg-indigo-500/10 px-2 py-1.5 text-xs font-medium text-indigo-300 transition-colors enabled:hover:border-indigo-500/60 enabled:hover:bg-indigo-500/20 enabled:hover:text-indigo-200 disabled:opacity-50 cursor-default"
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

        <div className="my-5 border-t border-zinc-800/80" />

        <TasksSection workspace={workspace} onError={onError} />

        <div className="my-5 border-t border-zinc-800/80" />

        <ScriptsSection workspace={workspace} onError={onError} />
      </div>
    </section>
  );
}
