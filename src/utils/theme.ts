/**
 * Theme selection.
 *
 * The database is the source of truth — the theme lives in app_settings, so it
 * travels with a backup like every other preference. But reading it is an async
 * round trip through Tauri, and a theme applied after React has mounted is a
 * visible flash of the wrong colours on every launch.
 *
 * So the choice is mirrored into localStorage and read back synchronously
 * before the first paint, with the database value reconciled once it arrives.
 * localStorage is a cache here and never the authority: it is per-machine, it
 * is not in backups, and it can come back empty or throw outright.
 *
 * The mirror earns its place twice over, because Spotlight and the timer alert
 * are separate webviews that share this origin. They pick up the cached theme
 * on their own without either of them needing to touch the database.
 */

export const THEMES = [
  {
    id: "graphite",
    label: "Graphite",
    blurb: "The original. Cool grey, indigo accent.",
  },
  {
    id: "deep-field",
    label: "Deep field",
    blurb: "Cool near-black, warm ink, soft blue.",
  },
  {
    id: "ember",
    label: "Ember",
    blurb: "Warm near-black, serif titles, rules instead of boxes.",
  },
  {
    id: "almanac",
    label: "Almanac",
    blurb: "Monospaced and achromatic. Fits the most on screen.",
  },
] as const;

export type ThemeId = (typeof THEMES)[number]["id"];

/** Today's look. Default on purpose: an update must not restyle anyone's app. */
export const DEFAULT_THEME: ThemeId = "graphite";

export const THEME_SETTING_KEY = "theme";
const THEME_CACHE_KEY = "orion.theme";

export function isThemeId(value: unknown): value is ThemeId {
  return THEMES.some((t) => t.id === value);
}

/**
 * The theme to paint with right now, before anything has been loaded. Falls
 * back to the default rather than throwing: a private window, cleared site data
 * or a storage policy can all make this fail, and none of them is a reason to
 * fail to render.
 */
export function readCachedTheme(): ThemeId {
  try {
    const cached = localStorage.getItem(THEME_CACHE_KEY);
    if (isThemeId(cached)) return cached;
  } catch {
    // Storage unavailable; the default is a perfectly good answer.
  }
  return DEFAULT_THEME;
}

function cacheTheme(theme: ThemeId): void {
  try {
    localStorage.setItem(THEME_CACHE_KEY, theme);
  } catch {
    // Best-effort. The database still holds the real value.
  }
}

/**
 * Puts the theme on <html>, where tokens.css is waiting for it, and refreshes
 * the cache so the next launch paints correctly on the first frame.
 */
export function applyTheme(theme: ThemeId): void {
  document.documentElement.dataset.theme = theme;
  cacheTheme(theme);
}
