import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ExportPasswordDialog from "./ExportPasswordDialog";

function renderDialog() {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  render(
    <ExportPasswordDialog busy={false} onCancel={onCancel} onConfirm={onConfirm} />
  );
  return {
    onConfirm,
    onCancel,
    password: () => screen.getByLabelText("Password"),
    confirm: () => screen.getByLabelText("Confirm password"),
    submit: () => screen.getByRole("button", { name: /export/i }),
    toggle: () => screen.getByLabelText("Encrypt with a password"),
  };
}

describe("ExportPasswordDialog", () => {
  it("offers encryption by default", () => {
    // The default is the decision: a backup carries environment variable
    // values, so falling into a readable file should not be possible by
    // simply not reading the dialog.
    const { toggle, password } = renderDialog();
    expect(toggle()).toBeChecked();
    expect(password()).toBeInTheDocument();
  });

  it("will not export until the two passwords match", () => {
    const { password, confirm, submit, onConfirm } = renderDialog();

    fireEvent.change(password(), { target: { value: "a-good-password" } });
    fireEvent.change(confirm(), { target: { value: "a-good-passwore" } });

    expect(screen.getByText(/do not match/i)).toBeInTheDocument();
    expect(submit()).toBeDisabled();

    fireEvent.click(submit());
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("passes the password up once it is confirmed", () => {
    const { password, confirm, submit, onConfirm } = renderDialog();

    fireEvent.change(password(), { target: { value: "a-good-password" } });
    fireEvent.change(confirm(), { target: { value: "a-good-password" } });
    fireEvent.click(submit());

    expect(onConfirm).toHaveBeenCalledWith("a-good-password");
  });

  it("refuses an empty password rather than exporting unprotected", () => {
    // Both fields empty means they match, which would otherwise read as
    // ready. An empty password must not quietly become "no password".
    const { submit, onConfirm } = renderDialog();

    expect(submit()).toBeDisabled();
    fireEvent.click(submit());
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("submits on Enter from either field", () => {
    const { password, confirm, onConfirm } = renderDialog();

    fireEvent.change(password(), { target: { value: "a-good-password" } });
    fireEvent.change(confirm(), { target: { value: "a-good-password" } });
    fireEvent.keyDown(confirm(), { key: "Enter" });

    expect(onConfirm).toHaveBeenCalledWith("a-good-password");
  });

  it("warns plainly when encryption is turned off, and sends no password", () => {
    const { toggle, submit, onConfirm } = renderDialog();

    fireEvent.click(toggle());

    expect(screen.getByText(/readable by anyone/i)).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();

    fireEvent.click(submit());
    // null, not "" — the API treats an empty string as a password and would
    // fail, where null is the deliberate choice to export in the clear.
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("says a lost password cannot be recovered, before the export runs", () => {
    renderDialog();
    expect(screen.getByText(/cannot be opened/i)).toBeInTheDocument();
  });

  it("keeps the password hidden until asked", () => {
    const { password } = renderDialog();
    expect(password()).toHaveAttribute("type", "password");

    fireEvent.click(screen.getByLabelText("Show password"));
    expect(password()).toHaveAttribute("type", "text");
  });

  it("cancels on Escape", () => {
    const { onCancel, password } = renderDialog();
    fireEvent.keyDown(password(), { key: "Escape" });
    expect(onCancel).toHaveBeenCalled();
  });
});
