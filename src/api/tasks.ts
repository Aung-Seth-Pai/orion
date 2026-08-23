import { invoke } from "@tauri-apps/api/core";
import type { NewTaskInput, Task } from "../types";

export function getTasks(workspaceId: string): Promise<Task[]> {
  return invoke("get_tasks", { workspaceId });
}

export function createTask(input: NewTaskInput): Promise<Task> {
  return invoke("create_task", { input });
}

export function updateTaskStatus(id: string, isCompleted: boolean): Promise<Task> {
  return invoke("update_task_status", { id, isCompleted });
}

export function deleteTask(id: string): Promise<boolean> {
  return invoke("delete_task", { id });
}
