import { useState } from "react";
import {
  AlertTriangle,
  Download,
  Eye,
  EyeOff,
  Loader2,
  Lock,
  X,
} from "lucide-react";

interface ExportPasswordDialogProps {
  /** True while the export is running, which includes the key derivation. */
  busy: boolean;
  onCancel: () => void;
  /** `null` means the user deliberately chose a readable file. */
  onConfirm: (password: string | null) => void;
}

/**
 * Describes a password by its length and nothing else.
 *
 * No strength library: those score character classes, which rewards `P@ssw0rd`
 * over `four random words together` and gives confident-looking numbers for a
 * property they cannot actually measure. Length is the one honest signal, so it
 * is the only one offered.
 */
function describeLength(password: string): { text: string; tone: string } | null {
  if (!password) return null;
  if (password.length < 8) {
    return {
      text: "Short. A few unrelated words make a stronger password than a brief, complicated one.",
      tone: "text-caution-ink",
    };
  }
  if (password.length < 16) return { text: "Reasonable length.", tone: "text-ink-faint" };
  return { text: "Good length.", tone: "text-positive-ink" };
}

/**
 * Asks for a password before the native save dialog opens.
 *
 * Encryption is pre-selected rather than offered neutrally, because the default
 * matters: a backup carries the values of every environment variable, which is
 * where API tokens live. Leaving it readable is a legitimate choice — it is the
 * only way to inspect or repair a backup by hand — but it should be a choice
 * someone makes, not one they fall into.
 */
export default function ExportPasswordDialog({
  busy,
  onCancel,
  onConfirm,
}: ExportPasswordDialogProps) {
  const [encrypt, setEncrypt] = useState(true);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [reveal, setReveal] = useState(false);

  const mismatch = confirm.length > 0 && password !== confirm;
  const ready = encrypt ? password.length > 0 && password === confirm : true;
  const hint = describeLength(password);

  function submit() {
    if (!ready || busy) return;
    onConfirm(encrypt ? password : null);
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Export data backup"
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-void/70 backdrop-blur-sm"
    >
      <div className="mx-4 w-full max-w-md rounded-xl border border-rule-strong bg-surface p-5 shadow-2xl">
        <div className="mb-4 flex items-start gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent-ink">
            <Lock size={18} />
          </span>
          <div>
            <h3 className="text-sm font-semibold text-ink-strong">
              Protect this backup?
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-ink-muted">
              A backup includes the values of your environment variables. A
              password encrypts the whole file, so it can be carried on a USB
              stick or through a sync folder without those values being
              readable.
            </p>
          </div>
        </div>

        <label className="flex cursor-default items-center gap-2 text-xs text-ink-strong">
          <input
            type="checkbox"
            checked={encrypt}
            onChange={(e) => setEncrypt(e.target.checked)}
            className="accent-accent"
          />
          Encrypt with a password
        </label>

        {encrypt ? (
          <div className="mt-3 space-y-2">
            <div className="flex items-center gap-2">
              <input
                type={reveal ? "text" : "password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                }}
                placeholder="Password"
                aria-label="Password"
                maxLength={256}
                spellCheck={false}
                autoFocus
                className="min-w-0 flex-1 rounded bg-raised px-2 py-1.5 text-xs text-ink-strong placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50"
              />
              <button
                onClick={() => setReveal((r) => !r)}
                title={reveal ? "Hide password" : "Show password"}
                aria-label={reveal ? "Hide password" : "Show password"}
                className="shrink-0 rounded p-1 text-ink-faint transition-colors hover:text-ink"
              >
                {reveal ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>

            <input
              type={reveal ? "text" : "password"}
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="Confirm password"
              aria-label="Confirm password"
              maxLength={256}
              spellCheck={false}
              className="w-full rounded bg-raised px-2 py-1.5 text-xs text-ink-strong placeholder-ink-faint outline-none ring-1 ring-transparent focus:ring-focus/50"
            />

            {mismatch ? (
              <p className="text-[11px] text-danger-ink">
                The two passwords do not match.
              </p>
            ) : (
              hint && <p className={`text-[11px] ${hint.tone}`}>{hint.text}</p>
            )}

            {/*
              Said before the export, not after. There is no key escrow and no
              reset: the password is the only way in, and someone who learns
              that later has already lost the backup.
            */}
            <p className="flex items-start gap-1.5 text-[11px] leading-relaxed text-ink-faint">
              <AlertTriangle size={12} className="mt-0.5 shrink-0 text-caution-ink" />
              <span>
                Keep this password somewhere safe. If it is lost the backup
                cannot be opened — by you or by anyone else.
              </span>
            </p>
          </div>
        ) : (
          <p className="mt-3 flex items-start gap-1.5 text-[11px] leading-relaxed text-caution-ink">
            <AlertTriangle size={12} className="mt-0.5 shrink-0" />
            <span>
              The file will be readable by anyone who opens it, including the
              value of every environment variable you have saved.
            </span>
          </p>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="flex items-center gap-1 rounded bg-raised px-3 py-1.5 text-xs font-medium text-ink-muted transition-colors hover:bg-raised-hover"
          >
            <X size={12} />
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={!ready || busy}
            className="flex items-center gap-1.5 rounded bg-accent/90 px-3 py-1.5 text-xs font-medium text-accent-on transition-colors enabled:hover:bg-accent-bright disabled:cursor-not-allowed disabled:opacity-40"
          >
            {busy ? (
              <Loader2 size={13} className="animate-spin" />
            ) : (
              <Download size={13} />
            )}
            {encrypt ? "Encrypt and export" : "Export unprotected"}
          </button>
        </div>
      </div>
    </div>
  );
}
