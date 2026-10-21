import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import PomodoroHairline from "./PomodoroHairline";

/** The inner fill element, whose width carries the progress. */
function fillWidth(container: HTMLElement): string | undefined {
  const fill = container.firstElementChild?.firstElementChild;
  return (fill as HTMLElement | null | undefined)?.style.width;
}

describe("PomodoroHairline", () => {
  it("renders nothing when no pomodoro is running", () => {
    // A stopwatch has no end to be near, so there is deliberately no
    // indeterminate state here to fall back to.
    const { container } = render(<PomodoroHairline fraction={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("fills in proportion to progress", () => {
    const { container } = render(<PomodoroHairline fraction={0.25} />);
    expect(fillWidth(container)).toBe("25%");
  });

  it("clamps rather than overflowing the window", () => {
    // elapsed can tip past the configured length between the final tick and
    // the completion handler, which would otherwise paint a bar wider than the
    // window it sits on.
    expect(fillWidth(render(<PomodoroHairline fraction={1.4} />).container)).toBe(
      "100%"
    );
    expect(
      fillWidth(render(<PomodoroHairline fraction={-0.2} />).container)
    ).toBe("0%");
  });

  it("is hidden from assistive technology", () => {
    // It is an ambient restatement of the timer numerals, not new information,
    // so announcing it would just be the same fact read out twice.
    const { container } = render(<PomodoroHairline fraction={0.5} />);
    expect(container.firstElementChild).toHaveAttribute("aria-hidden");
  });
});
