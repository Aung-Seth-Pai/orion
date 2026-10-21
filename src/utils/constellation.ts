/**
 * Deterministic constellation marks: the same workspace always yields the same
 * figure, so you learn your workspaces by shape.
 *
 * This is the shipping implementation. `design/mark.js` is the design lab's
 * copy of the same algorithm, kept plain so the lab pages open in a browser
 * with no build step — and `constellation.parity.test.ts` executes that file
 * and asserts it produces identical geometry, so the two cannot drift apart
 * silently.
 */

/** FNV-1a. Small, fast, and enough spread for seeding. */
function hash(str: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** mulberry32 — tiny seeded PRNG. */
function rng(seed: number): () => number {
  let a = seed;
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * The ceiling below which nothing is fully lit, however little the user has
 * done. Without it a brand new install would crown whichever workspace
 * happened to be opened first.
 */
export const CEILING_FLOOR_HOURS = 20;

/** Nothing ever reads as broken or reproachful, only as not yet invested in. */
export const DEPTH_FLOOR = 0.16;

/**
 * Maps accumulated focus hours to a 0..1 depth, calibrated against the USER'S
 * OWN practice rather than against an absolute scale.
 *
 * An absolute ceiling is the obvious implementation and it is wrong. Pick 45
 * hours and a casual user whose largest workspace is three hours sees twelve
 * identical floor-level marks forever, while someone with two thousand hours
 * sees twelve identical saturated ones. The mark stops carrying information in
 * both directions, and which of those you get depends entirely on whose data
 * the constant was tuned against.
 */
export function depthFromHours(hours: number, maxHours: number): number {
  const ceiling = Math.max(maxHours || 0, CEILING_FLOOR_HOURS);
  const ratio = Math.max(0, hours || 0) / ceiling;
  // Compressed near the top: the difference between 2h and 10h should read
  // more loudly than the difference between 300h and 400h.
  const curved = Math.log1p(ratio * 9) / Math.log1p(9);
  return Math.max(DEPTH_FLOOR, Math.min(1, curved));
}

export interface Star {
  x: number;
  y: number;
  /** Brighter focal stars; a uniform field of identical dots reads as noise. */
  primary: boolean;
}

export interface Mark {
  stars: Star[];
  edges: Array<[number, number]>;
  strokeWidth: number;
  rPrimary: number;
  rSecondary: number;
  lineOpacity: number;
  starOpacity: number;
}

interface Point {
  x: number;
  y: number;
  mag: number;
}

/**
 * Stratified placement: one point per chosen grid cell, jittered inside it.
 * Uniform random clumps badly at this few points, which reads as a mistake
 * rather than as a constellation.
 */
function points(
  rand: () => number,
  count: number,
  cols: number,
  rows: number,
  jitter: number
): Point[] {
  const cells: Array<[number, number]> = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) cells.push([x, y]);
  }
  for (let i = cells.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [cells[i], cells[j]] = [cells[j], cells[i]];
  }
  const cw = 100 / cols;
  const ch = 100 / rows;
  return cells.slice(0, count).map(([cx, cy]) => ({
    x: cx * cw + cw / 2 + (rand() - 0.5) * cw * jitter,
    y: cy * ch + ch / 2 + (rand() - 0.5) * ch * jitter,
    mag: rand(),
  }));
}

/** Prim's MST. Every star connects, and edges do not needlessly cross. */
function mst(pts: Point[]): Array<[number, number]> {
  const edges: Array<[number, number]> = [];
  const inTree = new Set([0]);
  while (inTree.size < pts.length) {
    let best: { i: number; j: number; d: number } | null = null;
    for (const i of inTree) {
      for (let j = 0; j < pts.length; j++) {
        if (inTree.has(j)) continue;
        const d = Math.hypot(pts[i].x - pts[j].x, pts[i].y - pts[j].y);
        if (!best || d < best.d) best = { i, j, d };
      }
    }
    if (!best) break;
    edges.push([best.i, best.j]);
    inTree.add(best.j);
  }
  return edges;
}

export interface ConstellationOptions {
  cols?: number;
  rows?: number;
  jitter?: number;
  minStars?: number;
  maxStars?: number;
  pad?: number;
  fill?: number;
}

/**
 * @param seed  stable workspace identity. Callers pass the workspace ID, not
 *              its name, so renaming a workspace keeps its constellation.
 * @param depth 0..1 from depthFromHours()
 * @param size  the px size it will be DISPLAYED at, which drives stroke weights
 */
export function constellation(
  seed: string,
  depth: number,
  size = 22,
  opts: ConstellationOptions = {}
): Mark {
  const {
    cols = 3,
    rows = 2,
    jitter = 0.8,
    minStars = 4,
    maxStars = 6,
    pad = 11,
    // Fully stretching the figure to the edges snapped too many of them into
    // suspiciously axis-aligned Ls and brackets, which read as geometry rather
    // than sky. Pulling most of the way keeps the optical weight even without
    // flattening the shape.
    fill = 0.72,
  } = opts;

  const rand = rng(hash(String(seed)));
  const count = minStars + Math.floor(rand() * (maxStars - minStars + 1));
  const pts = points(rand, count, cols, rows, jitter);
  const edges = mst(pts);

  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  const span = (v: number) => (v < 1 ? 1 : v);
  const x0 = Math.min(...xs);
  const y0 = Math.min(...ys);
  const sx = (100 - pad * 2) / span(Math.max(...xs) - x0);
  const sy = (100 - pad * 2) / span(Math.max(...ys) - y0);
  const inset = (100 - pad * 2) / 100;
  for (const p of pts) {
    const nx = pad + (p.x - x0) * sx;
    const ny = pad + (p.y - y0) * sy;
    const ox = pad + p.x * inset;
    const oy = pad + p.y * inset;
    p.x = ox + (nx - ox) * fill;
    p.y = oy + (ny - oy) * fill;
  }

  // Optical sizing. The viewBox is 100 units wide however large the mark is
  // drawn, so a fixed stroke width renders a quarter of a pixel at 22px and a
  // clumsy slab at 86px. Weights are specified in RENDERED pixels and
  // converted, so the figure keeps the same visual weight at any size.
  const k = 100 / size;
  const strokePx = 0.95 + size / 150;
  const primaryPx = 1.45 + size / 90;
  const secondaryPx = 0.95 + size / 150;

  // Shape and investment are kept as SEPARATE meanings: the figure is always
  // drawn in full, and only its brightness scales with accumulated time. An
  // earlier version withheld the lines until a workspace had hours on it so
  // the figure would "emerge" with use; rendering a fresh install showed three
  // workspaces as three near-identical scatters of dots, at exactly the moment
  // a new user most needs to tell them apart.
  const lineOpacity = (0.3 + depth * 0.7) * 0.62;
  const starOpacity = 0.3 + depth * 0.7;

  return {
    stars: pts.map((p, idx) => ({
      x: p.x,
      y: p.y,
      primary: p.mag > 0.72 || idx === 0,
    })),
    edges,
    strokeWidth: strokePx * k,
    rPrimary: primaryPx * k,
    rSecondary: secondaryPx * k,
    lineOpacity,
    starOpacity,
  };
}
