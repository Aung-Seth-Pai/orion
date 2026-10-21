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
        <ListTodo size={13} className="text-ink-faint" />
        <h3 className="text-[11px] font-semibold tracking-wider text-ink-muted uppercase">
          Tasks
        </h3>
        <span className="rounded bg-raised px-1.5 py-0.5 text-[10px] text-ink-muted tabular-nums">
          {tasks.length > 0 ? `${doneCount}/${tasks.length}` : "0"}
        </span>
      </div>

      <div className="mb-2 flex items-center gap-2 rounded-md bg-surface/70 px-3 py-1.5 ring-1 ring-rule focus-within:ring-focus/40">
        <Plus size={13} className="shrink-0 text-ink-faint" />
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
          className="w-full bg-transparent text-[13px] text-ink placeholder-ink-faint outline-none"
        />
        {adding && <Loader2 size={12} className="animate-spin text-ink-faint" />}
      </div>

      {loading ? (
        <p className="py-3 text-center text-xs text-ink-faint">Loading tasks…</p>
      ) : tasks.length === 0 ? (
        <p className="rounded-lg border border-dashed border-rule py-4 text-center text-xs text-ink-faint">
          No tasks yet for this workspace.
        </p>
      ) : (
        <ul className="space-y-0.5">
          {tasks.map((task) => (
            <li key={task.id} className="group flex items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors hover:bg-raised/40">
              <button
                onClick={() => void handleToggle(task)}
                title={task.isCompleted ? "Mark as not done" : "Mark as done"}
                className={`flex size-4 shrink-0 items-center justify-center rounded border transition-colors ${
                  task.isCompleted
                    ? "border-positive bg-positive text-positive-on"
                    : "border-rule-strong hover:border-positive-ink"
                }`}
              >
                {task.isCompleted && <Check size={11} strokeWidth={3} />}
              </button>
              <span
                className={`min-w-0 flex-1 truncate text-[13px] ${
                  task.isCompleted ? "text-ink-faint line-through" : "text-ink-muted"
                }`}
              >
                {task.title}
              </span>
              <button
                onClick={() => void handleDelete(task.id)}
                title="Delete task"
                className="rounded p-0.5 text-ink-faint opacity-0 transition-opacity hover:text-danger-ink group-hover:opacity-100"
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
