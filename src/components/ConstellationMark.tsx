import { useMemo } from "react";
import { constellation } from "../utils/constellation";

interface ConstellationMarkProps {
  /** Stable identity — the workspace ID, so renaming keeps the figure. */
  seed: string;
  /** 0..1 from depthFromHours(). Drives brightness only, never shape. */
  depth: number;
  /** Rendered size in px. Drives optical stroke weights, so pass the real one. */
  size?: number;
  /** For screen readers; the mark is decorative when this is omitted. */
  title?: string;
}

/**
 * A workspace's constellation, drawn inline with no dependency and no network.
 *
 * `currentColor` throughout, so the mark inherits whatever the surrounding row
 * is already doing for active/hover/muted states and needs no theme awareness
 * of its own.
 */
export default function ConstellationMark({
  seed,
  depth,
  size = 22,
  title,
}: ConstellationMarkProps) {
  // Cheap, but it runs for every workspace on every sidebar render, including
  // each tick of a running timer.
  const mark = useMemo(
    () => constellation(seed, depth, size),
    [seed, depth, size]
  );

  return (
    <svg
      viewBox="0 0 100 100"
      width={size}
      height={size}
      role={title ? "img" : "presentation"}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      className="block shrink-0 overflow-visible"
    >
      {mark.edges.map(([i, j]) => (
        <line
          key={`${i}-${j}`}
          x1={mark.stars[i].x}
          y1={mark.stars[i].y}
          x2={mark.stars[j].x}
          y2={mark.stars[j].y}
          stroke="currentColor"
          strokeWidth={mark.strokeWidth}
          strokeLinecap="round"
          opacity={mark.lineOpacity}
        />
      ))}
      {mark.stars.map((star, idx) => (
        <circle
          key={idx}
          cx={star.x}
          cy={star.y}
          r={star.primary ? mark.rPrimary : mark.rSecondary}
          fill="currentColor"
          opacity={(star.primary ? 1 : 0.7) * mark.starOpacity}
        />
      ))}
    </svg>
  );
}
