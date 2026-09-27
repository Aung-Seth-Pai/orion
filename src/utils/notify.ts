import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

/**
 * Brief two-tone beep via the Web Audio API. Audible confirmation that does not
 * depend on the OS notification stack at all.
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
 * Attempts a native Windows toast. Returns null on success, or the reason it
 * failed.
 *
 * Errors are RETURNED rather than swallowed. This used to sit inside a bare
 * `catch {}`, which meant a permanently broken toast was indistinguishable from
 * a working one and the only symptom was silence — the reason the test button in
 * Settings exists.
 *
 * Note the honest limit of this check: Windows can accept a toast and still
 * never draw a banner (Focus Assist, a per-app setting, policy). Success here
 * means "Windows took it", not "the user saw it". That gap is exactly why the
 * in-app alert window is the primary notification.
 */
export async function sendNativeToast(
  title: string,
  body: string
): Promise<string | null> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (!granted) {
      return "Windows notification permission was not granted for Orion.";
    }
    // Sent from Rust rather than via sendNotification so the toast carries an
    // explicit icon path; Windows frequently suppresses iconless banners.
    await invoke("notify_timer_complete", { title, body });
    return null;
  } catch (e) {
    return String(e);
  }
}

/**
 * Shows Orion's own always-on-top alert window. This is the notification the
 * user is actually relying on, so its failure is reported to the caller.
 */
export async function showTimerAlert(
  title: string,
  body: string
): Promise<string | null> {
  try {
    await invoke("show_timer_alert", { title, body });
    return null;
  } catch (e) {
    return String(e);
  }
}

/**
 * Pomodoro completion feedback, in order of reliability: the beep always fires,
 * Orion's own alert window is shown, and a native toast is attempted alongside
 * it so the notification also lands in the Windows notification centre.
 *
 * Returns an error only when the alert window itself failed — a toast that
 * Windows refused is logged, not surfaced, because the user has already been
 * told by the window and the beep.
 */
export async function notifyPomodoroComplete(): Promise<string | null> {
  const title = "Pomodoro Complete";
  const body = "Session logged. Time for a short break!";

  playBeep();

  const [alertError, toastError] = await Promise.all([
    showTimerAlert(title, body),
    sendNativeToast(title, body),
  ]);

  if (toastError) {
    console.warn(`Orion: native toast failed — ${toastError}`);
  }
  return alertError;
}
