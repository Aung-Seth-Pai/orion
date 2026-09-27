// Deterministic constellation marks. Same name always yields the same figure.

/** FNV-1a. Small, fast, and enough spread for seeding. */
function hash(str) {
  let h = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** mulberry32 — tiny seeded PRNG. */
function rng(seed) {
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
 * Stratified placement: one point per chosen grid cell, jittered inside it.
 * Uniform random clumps badly at this few points, which reads as a mistake
 * rather than as a constellation.
 */
function points(rand, count, cols, rows, jitter) {
  const cells = [];
  for (let y = 0; y < rows; y++) for (let x = 0; x < cols; x++) cells.push([x, y]);
  for (let i = cells.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [cells[i], cells[j]] = [cells[j], cells[i]];
  }
  const cw = 100 / cols;
  const ch = 100 / rows;
  return cells.slice(0, count).map(([cx, cy]) => ({
    x: cx * cw + cw / 2 + (rand() - 0.5) * cw * jitter,
    y: cy * ch + ch / 2 + (rand() - 0.5) * ch * jitter,
    // A couple of brighter stars give the figure a focal point; a uniform
    // field of identical dots reads as noise.
    mag: rand(),
  }));
}

/** Prim's MST. Every star connects, and edges do not needlessly cross. */
function mst(pts) {
  const edges = [];
  const inTree = new Set([0]);
  while (inTree.size < pts.length) {
    let best = null;
    for (const i of inTree) {
      for (let j = 0; j < pts.length; j++) {
        if (inTree.has(j)) continue;
        const d = Math.hypot(pts[i].x - pts[j].x, pts[i].y - pts[j].y);
        if (!best || d < best.d) best = { i, j, d };
      }
    }
    edges.push([best.i, best.j]);
    inTree.add(best.j);
  }
  return edges;
}

/**
 * @param name  workspace name — the identity
 * @param depth 0..1 accumulated focus time, floored by the caller
 * @param size  the px size it will be DISPLAYED at, which drives stroke weights
 */
function constellation(name, depth, size = 22, opts = {}) {
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

  const rand = rng(hash(name));
  const count = minStars + Math.floor(rand() * (maxStars - minStars + 1));
  const pts = points(rand, count, cols, rows, jitter);
  const edges = mst(pts);

  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  const span = (v) => (v < 1 ? 1 : v);
  const x0 = Math.min(...xs), y0 = Math.min(...ys);
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
  // clumsy slab at 86px. Weights are specified in RENDERED pixels and converted,
  // so the figure keeps the same visual weight at any size — growing a little
  // with size so a large mark does not look anaemic.
  const k = 100 / size;
  const strokePx = 0.95 + size / 150;
  const primaryPx = 1.45 + size / 90;
  const secondaryPx = 0.95 + size / 150;

  const strokeW = (strokePx * k).toFixed(2);
  const rPrimary = (primaryPx * k).toFixed(2);
  const rSecondary = (secondaryPx * k).toFixed(2);

  // Lines resolve later than stars: a new workspace is scattered points with no
  // figure drawn yet, and the shape emerges as hours accumulate.
  const lineAlpha = Math.max(0, (depth - 0.22) / 0.78) * 0.7;
  const starAlpha = 0.3 + depth * 0.7;

  const parts = [];
  for (const [i, j] of edges) {
    if (lineAlpha < 0.02) break;
    parts.push(
      `<line x1="${pts[i].x.toFixed(2)}" y1="${pts[i].y.toFixed(2)}" x2="${pts[j].x.toFixed(2)}" y2="${pts[j].y.toFixed(2)}" stroke="currentColor" stroke-width="${strokeW}" stroke-linecap="round" opacity="${lineAlpha.toFixed(3)}"/>`
    );
  }
  pts.forEach((p, idx) => {
    const primary = p.mag > 0.72 || idx === 0;
    parts.push(
      `<circle cx="${p.x.toFixed(2)}" cy="${p.y.toFixed(2)}" r="${primary ? rPrimary : rSecondary}" fill="currentColor" opacity="${((primary ? 1 : 0.7) * starAlpha).toFixed(3)}"/>`
    );
  });

  return `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">${parts.join("")}</svg>`;
}
