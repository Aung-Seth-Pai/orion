interface PomodoroHairlineProps {
  /** 0..1 through the current pomodoro, or null when none is running. */
  fraction: number | null;
}

/**
 * A thin rule along the top edge of the window, filling as the pomodoro
 * runs.
 *
 * The point is to make elapsed time *felt* rather than read. The numerals in
 * the timer widget answer "how long exactly"; this answers "am I near the end"
 * at a glance, without the user having to look anywhere in particular — which
 * is the question actually being asked when someone glances up mid-task.
 *
 * Only pomodoros get one. A stopwatch has no end to be near, and an
 * indeterminate bar would be decoration pretending to be information.
 */
export default function PomodoroHairline({ fraction }: PomodoroHairlineProps) {
  if (fraction === null) return null;

  const pct = Math.min(Math.max(fraction, 0), 1) * 100;

  return (
    <div
      aria-hidden
      // The faint full-width track matters as much as the height. Without it
      // there was nothing on screen at all until the pomodoro was well under
      // way -- just an unexplained stub in the corner -- so the bar could not
      // be read as a proportion, which is the one thing it is for.
      className="pointer-events-none absolute inset-x-0 top-0 z-20 h-1 bg-accent/15"
    >
      <div
        className="h-full bg-accent"
        // Width is set inline because it changes every second: a Tailwind
        // class per percentage would generate a hundred of them.
        //
        // The transition is just longer than the one-second tick, so the fill
        // creeps continuously instead of stepping. It is explicitly not
        // reduced-motion-gated: this is the readout itself moving, not an
        // embellishment, and freezing it would make the bar jump instead.
        style={{ width: `${pct}%`, transition: "width 1.05s linear" }}
      />
    </div>
  );
}
