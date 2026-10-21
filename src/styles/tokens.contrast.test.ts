import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Read from disk rather than imported with ?raw: Vitest stubs CSS imports out,
// and ?raw on a stylesheet comes back as an empty string -- which would make
// every assertion below pass against nothing.
const tokensCss = readFileSync(resolve(__dirname, "tokens.css"), "utf8");

/**
 * Contrast is measured here rather than eyeballed, and measured against the
 * file that ships rather than against a copy of the values.
 *
 * It exists because the three new themes were originally LESS legible than the
 * one they were offered alongside — faint metadata sat near 2.5:1 — and nothing
 * in the design process would have caught that. Looking at a mock tells you
 * whether something is pretty, not whether it is readable.
 */

type Tokens = Record<string, string>;

function parseThemes(css: string): Record<string, Tokens> {
  // Comments carry prose like "not derivable from --accent: ..." which the
  // declaration pattern below would otherwise read as a declaration.
  const source = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const themes: Record<string, Tokens> = {};

  const blockPattern =
    /(?::root,\s*)?\[data-theme="([a-z-]+)"\]\s*\{([^}]*)\}/g;
  for (const [, name, body] of source.matchAll(blockPattern)) {
    const tokens: Tokens = {};
    for (const [, key, value] of body.matchAll(
      /(--[a-z-]+)\s*:\s*([^;]+);/g
    )) {
      tokens[key] = value.trim();
    }
    themes[name] = { ...(themes[name] ?? {}), ...tokens };
  }

  // Themes other than graphite only declare what they change, exactly as the
  // cascade applies them.
  for (const name of Object.keys(themes)) {
    if (name !== "graphite") {
      themes[name] = { ...themes.graphite, ...themes[name] };
    }
  }
  return themes;
}

function luminance(hex: string): number {
  const h = hex.trim().replace("#", "");
  const channels = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  const [r, g, b] = channels.map((c) =>
    c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(fg: string, bg: string): number {
  const a = luminance(fg);
  const b = luminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

const themes = parseThemes(tokensCss);
const THEME_NAMES = ["graphite", "deep-field", "ember", "almanac"] as const;

/** WCAG AA for body text. */
const AA = 4.5;

const SURFACES = ["--void", "--surface", "--raised"] as const;
const READABLE_INKS = ["--ink-strong", "--ink", "--ink-muted", "--ink-faint"];

/**
 * Graphite is the look Orion already ships, kept as the default so that no
 * existing install is restyled by an update. Measuring it turned up contrast
 * that predates this work, and quietly "fixing" it would break the promise the
 * theme exists to keep — so the shortfalls are recorded here with their real
 * numbers instead of being hidden by a laxer threshold.
 *
 * Nothing else may fail, and nothing may fail by more than it does today.
 */
const GRAPHITE_INHERITED: Record<string, number> = {
  "--ink-faint on --void": 4.12,
  "--ink-faint on --surface": 3.67,
  "--ink-faint on --raised": 3.08,
  "--accent-on on --accent": 4.47,
  "--positive-on on --positive": 2.54,
  "--danger-on on --danger": 3.76,
  "--caution-on on --caution": 2.15,
};

describe("the token file parses", () => {
  it("finds all four themes", () => {
    expect(Object.keys(themes).sort()).toEqual([...THEME_NAMES].sort());
  });
});

describe.each(THEME_NAMES)("%s", (name) => {
  const t = themes[name];

  it.each(READABLE_INKS)("%s meets AA on every surface", (ink) => {
    for (const surface of SURFACES) {
      const key = `${ink} on ${surface}`;
      const measured = contrast(t[ink], t[surface]);

      if (name === "graphite" && key in GRAPHITE_INHERITED) {
        // Allowed to stay as it is, not allowed to get worse.
        expect(measured).toBeGreaterThanOrEqual(
          GRAPHITE_INHERITED[key] - 0.01
        );
        continue;
      }
      expect(measured, `${name}: ${key}`).toBeGreaterThanOrEqual(AA);
    }
  });

  it.each([
    ["--accent-on", "--accent"],
    ["--positive-on", "--positive"],
    ["--danger-on", "--danger"],
    ["--caution-on", "--caution"],
  ])("%s is legible on %s", (fg, fill) => {
    const key = `${fg} on ${fill}`;
    const measured = contrast(t[fg], t[fill]);

    if (name === "graphite" && key in GRAPHITE_INHERITED) {
      expect(measured).toBeGreaterThanOrEqual(GRAPHITE_INHERITED[key] - 0.01);
      return;
    }
    expect(measured, `${name}: ${key}`).toBeGreaterThanOrEqual(AA);
  });

  it("keeps the ink hierarchy visibly stepped", () => {
    // Raising faint text to meet AA can quietly collapse it into the tier
    // above, which costs the hierarchy its meaning while passing every check.
    const on = (token: string) => contrast(t[token], t["--raised"]);
    expect(on("--ink")).toBeGreaterThan(on("--ink-muted") * 1.3);
    expect(on("--ink-muted")).toBeGreaterThan(on("--ink-faint") * 1.3);
  });

  it("defines every token the bridge exposes", () => {
    // A theme missing a token silently inherits graphite's, which reads as a
    // single stray colour rather than as an obvious mistake.
    for (const token of [
      ...SURFACES,
      ...READABLE_INKS,
      "--ink-ghost",
      "--rule",
      "--rule-strong",
      "--accent",
      "--accent-bright",
      "--accent-ink",
      "--accent-soft",
      "--accent-on",
      "--focus",
      "--selection",
      "--positive",
      "--positive-ink",
      "--positive-on",
      "--danger",
      "--danger-ink",
      "--danger-on",
      "--caution",
      "--caution-ink",
      "--caution-on",
      "--info-ink",
    ]) {
      expect(t[token], `${name} is missing ${token}`).toBeTruthy();
    }
  });
});
