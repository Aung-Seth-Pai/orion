import { useEffect, useState } from "react";
import { Check, ListTodo, Loader2, Plus, Trash2 } from "lucide-react";
import { createTask, deleteTask, getTasks, updateTaskStatus } from "../api/tasks";
import type { Task, Workspace } from "../types";

interface TasksSectionProps {
  workspace: Workspace;
  onError: (message: string | null) => void;
}

export default function TasksSection({ workspace, onError }: TasksSectionProps) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [title, setTitle] = useState("");
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getTasks(workspace.id)
      .then((list) => !cancelled && setTasks(list))
      .catch((e) => !cancelled && onError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id]);

  async function handleAdd() {
    const trimmed = title.trim();
    if (!trimmed || adding) return;
    onError(null);
    setAdding(true);
    try {
      const task = await createTask({ workspaceId: workspace.id, title: trimmed });
      setTasks((prev) => [...prev, task]);
      setTitle("");
    } catch (e) {
      onError(String(e));
    } finally {
      setAdding(false);
    }
  }

  async function handleToggle(task: Task) {
    onError(null);
    const next = !task.isCompleted;
    // Optimistic flip; move completed items to the bottom like the backend ordering.
    setTasks((prev) => {
      const updated = prev.map((t) => (t.id === task.id ? { ...t, isCompleted: next } : t));
      return [...updated.filter((t) => !t.isCompleted), ...updated.filter((t) => t.isCompleted)];
    });
    try {
      await updateTaskStatus(task.id, next);
    } catch (e) {
      onError(String(e));
      setTasks((prev) =>
        prev.map((t) => (t.id === task.id ? { ...t, isCompleted: task.isCompleted } : t))
      );
    }
  }

  async function handleDelete(id: string) {
    onError(null);
    const snapshot = tasks;
    setTasks((prev) => prev.filter((t) => t.id !== id));
    try {
      await deleteTask(id);
    } catch (e) {
      onError(String(e));
      setTasks(snapshot);
    }
  }

  const doneCount = tasks.filter((t) => t.isCompleted).length;

  return (
    <section>
      <div className="mb-2 flex items-center gap-2">
        <ListTodo size={13} className="text-zinc-500" />
        <h3 className="text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">
          Tasks
        </h3>
        <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400 tabular-nums">
          {tasks.length > 0 ? `${doneCount}/${tasks.length}` : "0"}
        </span>
      </div>

      <div className="mb-2 flex items-center gap-2 rounded-md bg-zinc-900/70 px-3 py-1.5 ring-1 ring-zinc-800 focus-within:ring-indigo-500/40">
        <Plus size={13} className="shrink-0 text-zinc-600" />
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void handleAdd();
            }
          }}
          maxLength={120}
          placeholder="Add a task and press Enter…"
          className="w-full bg-transparent text-[13px] text-zinc-200 placeholder-zinc-600 outline-none"
        />
        {adding && <Loader2 size={12} className="animate-spin text-zinc-500" />}
      </div>

      {loading ? (
        <p className="py-3 text-center text-xs text-zinc-600">Loading tasks…</p>
      ) : tasks.length === 0 ? (
        <p className="rounded-lg border border-dashed border-zinc-800 py-4 text-center text-xs text-zinc-600">
          No tasks yet for this workspace.
        </p>
      ) : (
        <ul className="space-y-0.5">
          {tasks.map((task) => (
            <li key={task.id} className="group flex items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors hover:bg-zinc-800/40">
              <button
                onClick={() => void handleToggle(task)}
                title={task.isCompleted ? "Mark as not done" : "Mark as done"}
                className={`flex size-4 shrink-0 items-center justify-center rounded border transition-colors ${
                  task.isCompleted
                    ? "border-emerald-500 bg-emerald-500 text-white"
                    : "border-zinc-600 hover:border-emerald-400"
                }`}
              >
                {task.isCompleted && <Check size={11} strokeWidth={3} />}
              </button>
              <span
                className={`min-w-0 flex-1 truncate text-[13px] ${
                  task.isCompleted ? "text-zinc-600 line-through" : "text-zinc-300"
                }`}
              >
                {task.title}
              </span>
              <button
                onClick={() => void handleDelete(task.id)}
                title="Delete task"
                className="rounded p-0.5 text-zinc-600 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100"
              >
                <Trash2 size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
