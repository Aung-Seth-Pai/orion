import { useState } from "react";
import { Check } from "lucide-react";
import { updateSetting } from "../api/settings";
import {
  applyTheme,
  readCachedTheme,
  THEME_SETTING_KEY,
  THEMES,
  type ThemeId,
} from "../utils/theme";

interface ThemePickerProps {
  onError: (message: string) => void;
}

export default function ThemePicker({ onError }: ThemePickerProps) {
  const [theme, setTheme] = useState<ThemeId>(readCachedTheme);

  async function choose(next: ThemeId) {
    if (next === theme) return;
    const previous = theme;

    // Applied before the write, so the click feels instant. The database is
    // the source of truth but it is a round trip away, and a theme that waits
    // on it feels broken even when it is working.
    setTheme(next);
    applyTheme(next);

    try {
      await updateSetting(THEME_SETTING_KEY, next);
    } catch (e) {
      // Put it back rather than leave the screen disagreeing with what is
      // stored: the next launch reads the database and would silently revert.
      setTheme(previous);
      applyTheme(previous);
      onError(`Could not save the theme: ${String(e)}`);
    }
  }

  return (
    <div className="rounded-lg border border-rule bg-surface/70 p-4">
      <p className="text-[13px] font-medium text-ink">Theme</p>
      <p className="mt-0.5 text-xs leading-relaxed text-ink-faint">
        Applies everywhere, including Quick Launch and the timer alert.
      </p>

      <div className="mt-3 grid grid-cols-2 gap-2">
        {THEMES.map((t) => {
          const active = t.id === theme;
          return (
            <button
              key={t.id}
              onClick={() => void choose(t.id)}
              aria-pressed={active}
              className={`flex cursor-default items-center gap-3 rounded-md border p-2.5 text-left transition-colors ${
                active
                  ? "border-accent/70 bg-accent/10"
                  : "border-rule hover:border-rule-strong hover:bg-raised/40"
              }`}
            >
              {/* Rendered under the candidate theme so each preview shows that
                  theme's own values rather than a hand-maintained copy that
                  could drift from tokens.css.

                  These MUST use raw var(--token) rather than Tailwind utility
                  classes. Tailwind's --color-* aliases are declared once at
                  :root as --color-x: var(--x), which CSS flattens there and
                  then inherits as a fixed value — so bg-void inside this span
                  would paint the ACTIVE theme's background, identically for all
                  four. A raw var() is substituted on the element that uses it,
                  where data-theme is in effect. */}
              <span
                aria-hidden
                data-theme={t.id}
                className="flex size-8 shrink-0 items-center justify-center rounded border"
                style={{
                  backgroundColor: "var(--void)",
                  borderColor: "var(--rule-strong)",
                }}
              >
                <span className="flex gap-0.5">
                  <span
                    className="size-1.5 rounded-full"
                    style={{ backgroundColor: "var(--raised)" }}
                  />
                  <span
                    className="size-1.5 rounded-full"
                    style={{ backgroundColor: "var(--ink-muted)" }}
                  />
                  <span
                    className="size-1.5 rounded-full"
                    style={{ backgroundColor: "var(--accent)" }}
                  />
                </span>
              </span>

              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5">
                  <span className="truncate text-xs font-medium text-ink">
                    {t.label}
                  </span>
                  {active && (
                    <Check size={12} className="shrink-0 text-accent-ink" />
                  )}
                </span>
                <span className="mt-0.5 block text-[11px] leading-snug text-ink-faint">
                  {t.blurb}
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
