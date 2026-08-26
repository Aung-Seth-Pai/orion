import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

/**
 * Brief two-tone beep via the Web Audio API. Used as an audible fallback
 * because native toasts can be blocked (e.g. Windows dev builds without an
 * installed Start Menu shortcut / AUMID).
 */
function playBeep(): void {
  try {
    const AudioCtx =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext })
        .webkitAudioContext;
    if (!AudioCtx) return;

    const ctx = new AudioCtx();
    const gain = ctx.createGain();
    gain.gain.setValueAtTime(0.12, ctx.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.45);
    gain.connect(ctx.destination);

    const first = ctx.createOscillator();
    first.type = "sine";
    first.frequency.value = 880;
    first.connect(gain);
    first.start();
    first.stop(ctx.currentTime + 0.2);

    const second = ctx.createOscillator();
    second.type = "sine";
    second.frequency.value = 1174.7; // D6
    second.connect(gain);
    second.start(ctx.currentTime + 0.22);
    second.stop(ctx.currentTime + 0.45);

    first.onended = () => void ctx.close().catch(() => {});
  } catch {
    // Audio is best-effort feedback only.
  }
}

/**
 * Pomodoro completion feedback: attempts a native OS toast and ALWAYS plays
 * the in-app beep so the user is notified even when the OS blocks toasts.
 */
export async function notifyPomodoroComplete(): Promise<void> {
  playBeep();
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (granted) {
      // Sent from Rust rather than via sendNotification so the toast carries an
      // explicit icon path; Windows frequently suppresses iconless banners.
      await invoke("notify_timer_complete", {
        title: "Pomodoro Complete",
        body: "Session logged. Time for a short break!",
      });
    }
  } catch {
    // Native notifications are best-effort; the beep already fired.
  }
}
