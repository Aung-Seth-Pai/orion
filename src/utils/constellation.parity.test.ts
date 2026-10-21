import { describe, expect, it } from "vitest";
// Imported as text through Vite rather than read from the filesystem, so this
// file needs no Node type declarations and still type-checks in the app build.
import labSource from "../../design/mark.js?raw";
import {
  CEILING_FLOOR_HOURS,
  constellation,
  depthFromHours,
} from "./constellation";

/**
 * The design lab keeps its own plain-JavaScript copy of the generator so its
 * pages open in a browser with no build step. Two copies of an algorithm drift,
 * and a mark that differs between the style guide and the app is worse than no
 * style guide at all — so rather than trusting a comment telling people to keep
 * them in sync, this executes the lab's file and compares the output.
 */
function loadLabGenerator(): {
  constellation: (seed: string, depth: number, size?: number) => string;
  depthFromHours: (hours: number, maxHours: number) => number;
} {
  // mark.js declares plain top-level functions for a <script> tag, so it is
  // evaluated and its bindings handed back explicitly.
  const factory = new Function(
    `${labSource}\nreturn { constellation, depthFromHours };`
  );
  return factory() as ReturnType<typeof loadLabGenerator>;
}

const lab = loadLabGenerator();

/** Parses the lab's SVG string back into comparable numbers. */
function parseLabSvg(svg: string) {
  const lines = [...svg.matchAll(/<line ([^/]+)\/>/g)].map((m) => m[1]);
  const circles = [...svg.matchAll(/<circle ([^/]+)\/>/g)].map((m) => m[1]);
  const attr = (s: string, name: string) =>
    Number(new RegExp(`${name}="([^"]+)"`).exec(s)?.[1]);
  return {
    lines: lines.map((l) => ({
      x1: attr(l, "x1"),
      y1: attr(l, "y1"),
      x2: attr(l, "x2"),
      y2: attr(l, "y2"),
      opacity: attr(l, "opacity"),
      strokeWidth: attr(l, "stroke-width"),
    })),
    circles: circles.map((c) => ({
      cx: attr(c, "cx"),
      cy: attr(c, "cy"),
      r: attr(c, "r"),
      opacity: attr(c, "opacity"),
    })),
  };
}

const SEEDS = [
  "w1",
  "workspace-0193f2ac-7b1d-4c2e-9f1a-5e8d3c7b2a16",
  "SE learning",
  "မြန်မာစာ",
  "",
  "X",
];
const SIZES = [18, 21, 22, 86];

describe("the lab copy and the shipping copy agree", () => {
  it("produces the same depth for the same hours", () => {
    for (const max of [0, 3, 41, 2410]) {
      for (const hours of [0, 0.5, 2, 20, 41, 2410]) {
        expect(depthFromHours(hours, max)).toBeCloseTo(
          lab.depthFromHours(hours, max),
          12
        );
      }
    }
  });

  it("produces the same geometry for the same seed", () => {
    for (const seed of SEEDS) {
      for (const size of SIZES) {
        const depth = 0.6;
        const mine = constellation(seed, depth, size);
        const theirs = parseLabSvg(lab.constellation(seed, depth, size));

        expect(theirs.circles).toHaveLength(mine.stars.length);
        expect(theirs.lines).toHaveLength(mine.edges.length);

        // The lab rounds to two decimals on the way into the SVG string, so
        // that is the precision the comparison can assert at.
        mine.stars.forEach((star, i) => {
          expect(theirs.circles[i].cx).toBeCloseTo(star.x, 2);
          expect(theirs.circles[i].cy).toBeCloseTo(star.y, 2);
          expect(theirs.circles[i].r).toBeCloseTo(
            star.primary ? mine.rPrimary : mine.rSecondary,
            2
          );
        });

        mine.edges.forEach(([a, b], i) => {
          expect(theirs.lines[i].x1).toBeCloseTo(mine.stars[a].x, 2);
          expect(theirs.lines[i].y1).toBeCloseTo(mine.stars[a].y, 2);
          expect(theirs.lines[i].x2).toBeCloseTo(mine.stars[b].x, 2);
          expect(theirs.lines[i].y2).toBeCloseTo(mine.stars[b].y, 2);
          expect(theirs.lines[i].strokeWidth).toBeCloseTo(mine.strokeWidth, 2);
        });
      }
    }
  });
});

describe("constellation", () => {
  it("is deterministic", () => {
    const a = constellation("w1", 0.5, 22);
    const b = constellation("w1", 0.5, 22);
    expect(b).toEqual(a);
  });

  it("gives different workspaces different figures", () => {
    const a = constellation("w1", 0.5, 22);
    const b = constellation("w2", 0.5, 22);
    expect(b.stars).not.toEqual(a.stars);
  });

  it("keeps the shape when only the hours change", () => {
    // Shape is identity, brightness is investment. Logging time must not
    // rearrange a figure the user has learned to recognise.
    const newWorkspace = constellation("w1", 0.16, 22);
    const wellUsed = constellation("w1", 1, 22);
    expect(wellUsed.stars.map((s) => [s.x, s.y])).toEqual(
      newWorkspace.stars.map((s) => [s.x, s.y])
    );
    expect(wellUsed.edges).toEqual(newWorkspace.edges);
    expect(wellUsed.starOpacity).toBeGreaterThan(newWorkspace.starOpacity);
  });

  it("draws the full figure even at the floor", () => {
    // A brand new install must still show a distinguishable shape; withholding
    // the lines until a workspace had hours on it made every new workspace
    // look alike.
    const mark = constellation("w1", depthFromHours(0, 0), 22);
    expect(mark.edges.length).toBe(mark.stars.length - 1);
    expect(mark.lineOpacity).toBeGreaterThan(0.05);
  });

  it("scales stroke weight so the mark reads at any size", () => {
    // The viewBox is always 100 units, so the stroke must shrink as the mark
    // grows or a 22px mark renders a quarter of a pixel wide.
    const small = constellation("w1", 0.5, 18);
    const large = constellation("w1", 0.5, 86);
    expect(small.strokeWidth).toBeGreaterThan(large.strokeWidth);
  });

  it("keeps every star inside the viewBox", () => {
    for (const seed of SEEDS) {
      for (const star of constellation(seed, 1, 22).stars) {
        expect(star.x).toBeGreaterThanOrEqual(0);
        expect(star.x).toBeLessThanOrEqual(100);
        expect(star.y).toBeGreaterThanOrEqual(0);
        expect(star.y).toBeLessThanOrEqual(100);
      }
    }
  });
});

describe("depthFromHours", () => {
  it("floors a brand new workspace above zero", () => {
    expect(depthFromHours(0, 0)).toBeGreaterThan(0);
  });

  it("gives a fresh install no fully-lit workspace", () => {
    // Everything the user has is three hours old. Nothing should read as an
    // achievement yet, and no workspace should be crowned for being first.
    const depths = [3, 2, 1].map((h) => depthFromHours(h, 3));
    for (const d of depths) expect(d).toBeLessThan(0.6);
  });

  it("still separates workspaces after thousands of hours", () => {
    // The failure the absolute ceiling caused: everything pinned at 1.
    const top = depthFromHours(2410, 2410);
    const middle = depthFromHours(390, 2410);
    const low = depthFromHours(30, 2410);
    expect(top).toBeCloseTo(1, 5);
    expect(middle).toBeLessThan(top - 0.1);
    expect(low).toBeLessThan(middle - 0.1);
  });

  it("treats the busiest workspace as the ceiling once past the floor", () => {
    expect(depthFromHours(50, 50)).toBeCloseTo(1, 5);
    expect(depthFromHours(2000, 2000)).toBeCloseTo(1, 5);
  });

  it("does not let a tiny maximum saturate everything", () => {
    expect(depthFromHours(1, 1)).toBeLessThan(
      depthFromHours(CEILING_FLOOR_HOURS, CEILING_FLOOR_HOURS)
    );
  });

  it("survives absent and negative input", () => {
    for (const d of [
      depthFromHours(-5, 10),
      depthFromHours(NaN, 10),
      depthFromHours(0, NaN),
    ]) {
      expect(Number.isFinite(d)).toBe(true);
      expect(d).toBeGreaterThan(0);
      expect(d).toBeLessThanOrEqual(1);
    }
  });
});
