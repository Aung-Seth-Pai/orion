import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ResourceSuggestions from "./ResourceSuggestions";
import type { ResourceSuggestion } from "../types";

function suggestion(over: Partial<ResourceSuggestion> = {}): ResourceSuggestion {
  return {
    id: "r1",
    type: "link",
    title: "Stripe API docs",
    targetPath: "https://stripe.com/docs",
    workspaceId: "w1",
    workspaceName: "Payments",
    matchKind: "keyword",
    ...over,
  };
}

describe("ResourceSuggestions", () => {
  it("renders nothing when there is nothing to say", () => {
    // It sits inside the add-resource form, so an empty state has to occupy no
    // space at all rather than leave a gap.
    const { container } = render(
      <ResourceSuggestions duplicate={null} similar={[]} onOpen={() => {}} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("states an exact duplicate as a fact, naming where it lives", () => {
    render(
      <ResourceSuggestion_Harness
        duplicate={suggestion({ matchKind: "exact" })}
        similar={[]}
      />
    );
    expect(screen.getByText(/already saved this/i)).toBeInTheDocument();
    expect(screen.getByText("Stripe API docs")).toBeInTheDocument();
    // The workspace is the useful part when the duplicate is elsewhere.
    expect(screen.getByText("Payments")).toBeInTheDocument();
  });

  it("presents resemblances as a question, not a warning", () => {
    render(
      <ResourceSuggestion_Harness
        duplicate={null}
        similar={[suggestion(), suggestion({ id: "r2", title: "Runbook" })]}
      />
    );
    expect(screen.getByText(/similar\?/i)).toBeInTheDocument();
    expect(screen.getByText("Runbook")).toBeInTheDocument();
    // And it must not claim the stronger thing.
    expect(screen.queryByText(/already saved this/i)).not.toBeInTheDocument();
  });

  it("shows a local file as a path rather than a file:// URL", () => {
    render(
      <ResourceSuggestion_Harness
        duplicate={null}
        similar={[
          suggestion({
            targetPath: "file:///C:/Users/zepha/Downloads/Resume.pdf",
          }),
        ]}
      />
    );
    expect(
      screen.getByText("C:\\Users\\zepha\\Downloads\\Resume.pdf")
    ).toBeInTheDocument();
  });

  it("marks the matches only semantic search could have found", () => {
    render(
      <ResourceSuggestion_Harness
        duplicate={null}
        similar={[
          suggestion({ matchKind: "semantic", title: "Amazon Web Services" }),
          suggestion({ id: "r2", matchKind: "keyword", title: "Runbook" }),
        ]}
      />
    );
    // Exactly one badge: the keyword hit needs no explaining.
    expect(screen.getAllByTitle("Found by meaning")).toHaveLength(1);
  });

  it("hands back the suggestion that was clicked, so the caller can jump to it", async () => {
    const onOpen = vi.fn();
    const target = suggestion({ id: "r2", workspaceId: "w9", title: "Runbook" });
    render(
      <ResourceSuggestions
        duplicate={null}
        similar={[suggestion(), target]}
        onOpen={onOpen}
      />
    );

    await userEvent.click(screen.getByText("Runbook"));
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onOpen).toHaveBeenCalledWith(target);
  });
});

/** Renders with a no-op handler, for the cases that do not exercise clicking. */
function ResourceSuggestion_Harness(props: {
  duplicate: ResourceSuggestion | null;
  similar: ResourceSuggestion[];
}) {
  return <ResourceSuggestions {...props} onOpen={() => {}} />;
}
