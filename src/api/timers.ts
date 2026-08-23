import { invoke } from "@tauri-apps/api/core";
import type { NewTimerLogInput } from "../types";

export function logTimerSession(input: NewTimerLogInput) {
  return invoke("log_timer_session", { input });
}
