import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import Sidebar from "./Sidebar";
import type { Workspace } from "../types";

function workspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: "w1",
    name: "Payments",
    color: null,
    icon: null,
    sortOrder: 0,
    ...overrides,
  } as Workspace;
}

/**
 * Renders the sidebar with one workspace and returns the rename spy.
 *
 * Only the rename path is exercised here; the rest of the sidebar is covered by
 * the behaviour it shares with every other list in the app.
 */
function renderSidebar(ws = workspace()) {
  const onRename = vi.fn();
  render(
    <Sidebar
      workspaces={[ws]}
      workspaceTimes={{}}
      activeId={ws.id}
      settingsActive={false}
      onSelect={vi.fn()}
      onOpenSettings={vi.fn()}
      onCreate={vi.fn()}
      onRename={onRename}
      onDelete={vi.fn()}
      onReorder={vi.fn()}
    />
  );
  return { onRename };
}

/** Opens the editor the way the pencil affordance does. */
function startEditing() {
  fireEvent.click(screen.getByTitle("Rename workspace"));
  return screen.getByDisplayValue("Payments");
}

describe("Sidebar renaming", () => {
  it("commits a new name on Enter", () => {
    const { onRename } = renderSidebar();
    const input = startEditing();

    fireEvent.change(input, { target: { value: "Billing" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).toHaveBeenCalledWith("w1", "Billing");
  });

  it("abandons the edit on Escape", () => {
    const { onRename } = renderSidebar();
    const input = startEditing();

    fireEvent.change(input, { target: { value: "Billing" } });
    fireEvent.keyDown(input, { key: "Escape" });

    // Escape and Enter both blur the field, so this is the case that proves
    // the two are actually told apart rather than both committing.
    expect(onRename).not.toHaveBeenCalled();
    expect(screen.getByText("Payments")).toBeInTheDocument();
  });

  it("does not call the backend when the name is unchanged", () => {
    const { onRename } = renderSidebar();
    const input = startEditing();

    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).not.toHaveBeenCalled();
  });

  it("treats a blanked name as a cancelled edit", () => {
    // The backend rejects an empty name. Sending it anyway would mean a round
    // trip to be told something already known, surfaced as an error banner for
    // what is really just an abandoned edit.
    const { onRename } = renderSidebar();
    const input = startEditing();

    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).not.toHaveBeenCalled();
    expect(screen.getByText("Payments")).toBeInTheDocument();
  });

  it("trims what it sends", () => {
    const { onRename } = renderSidebar();
    const input = startEditing();

    fireEvent.change(input, { target: { value: "  Billing  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).toHaveBeenCalledWith("w1", "Billing");
  });

  it("opens the editor on a double-click as well", () => {
    renderSidebar();
    fireEvent.doubleClick(screen.getByText("Payments"));
    expect(screen.getByDisplayValue("Payments")).toBeInTheDocument();
  });

  it("stops the row being draggable while it is being edited", () => {
    // An HTML5 drag source swallows the text selection a rename needs, so the
    // list item drops its drag props for as long as the editor is open.
    renderSidebar();
    const row = screen.getByText("Payments").closest("li");
    expect(row).toHaveAttribute("draggable", "true");

    startEditing();
    expect(screen.getByDisplayValue("Payments").closest("li")).not.toHaveAttribute(
      "draggable"
    );
  });
});
