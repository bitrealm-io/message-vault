/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import DeleteAccountDialog from "./DeleteAccountDialog";

afterEach(cleanup);

const deleteButton = () => screen.getByRole("button", { name: "Permanently delete my account" });

describe("DeleteAccountDialog", () => {
  it("asks an account with no password for its username only", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(
      <DeleteAccountDialog
        open
        username="carol"
        hasPassword={false}
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    expect(screen.queryByLabelText("Current password")).toBeNull();
    expect(deleteButton()).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: /Type your username carol/ }), "carol");
    expect(deleteButton()).toBeEnabled();
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledWith(undefined);
  });

  it("requires the current password from an account that has one", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(
      <DeleteAccountDialog
        open
        username="carol"
        hasPassword
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    await user.type(screen.getByRole("textbox", { name: /Type your username carol/ }), "carol");
    expect(deleteButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Current password"), "hunter2");
    expect(deleteButton()).toBeEnabled();
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledWith("hunter2");
  });
});
