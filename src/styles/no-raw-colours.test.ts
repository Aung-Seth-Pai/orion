import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * The token layer only holds if nothing bypasses it.
 *
 * Migrating 412 literals is a one-off; keeping them out is not. A single
 * `bg-zinc-800` added later looks fine in the theme it was written against and
 * is invisible in review — it only shows up as one stubborn grey patch when
 * somebody switches theme, which is exactly the kind of bug nobody files.
 */

const SRC = resolve(__dirname, "..");

/** Tailwind palette families that must not appear outside tokens.css. */
const RAW_COLOUR = new RegExp(
  String.raw`\b(?:bg|text|border|ring|from|to|via|fill|stroke|divide|outline|accent|decoration|placeholder)-` +
    String.raw`(?:zinc|slate|neutral|gray|stone|indigo|violet|purple|blue|sky|cyan|teal|emerald|green|lime|` +
    String.raw`yellow|amber|orange|red|rose|pink|fuchsia|white|black)` +
    String.raw`(?:-\d{2,3})?\b`,
  "g"
);

/**
 * A drop shadow is darkness rather than a palette choice: it reads correctly on
 * every one of these themes, and routing it through a token would invite the
 * idea that a theme might want a light one.
 */
const ALLOWED = [/\bshadow-black(?:\/\d{1,3})?\b/g];

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    if (!/\.tsx?$/.test(entry)) return [];
    // Tests name colours on purpose — the contrast suite is built from them.
    if (/\.test\.tsx?$/.test(entry)) return [];
    return [path];
  });
}

describe("no raw palette colours outside the token layer", () => {
  const files = sourceFiles(SRC);

  it("finds the source tree", () => {
    expect(files.length).toBeGreaterThan(10);
  });

  it.each(files.map((f) => [f.slice(SRC.length + 1), f]))(
    "%s",
    (_label, path) => {
      let source = readFileSync(path, "utf8");
      for (const allowed of ALLOWED) source = source.replace(allowed, "");

      const found = [...new Set(source.match(RAW_COLOUR) ?? [])];
      expect(
        found,
        `use a design token instead — see src/styles/tokens.css`
      ).toEqual([]);
    }
  );
});
