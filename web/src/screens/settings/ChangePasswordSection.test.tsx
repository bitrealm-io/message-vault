/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ChangePasswordSection } from "./ChangePasswordSection";

const changePassword = vi.hoisted(() => vi.fn());
const updateToken = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ updateToken }),
}));

vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  changePassword: (...a: unknown[]) => changePassword(...a),
}));

beforeEach(() => {
  changePassword.mockReset();
  updateToken.mockReset();
  changePassword.mockResolvedValue({ token: "mv-user-rotated" });
});

afterEach(cleanup);

describe("ChangePasswordSection", () => {
  it("asks for the new password twice and never the current one", () => {
    render(<ChangePasswordSection />);

    expect(screen.getByLabelText("New password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm new password")).toBeInTheDocument();
    expect(screen.queryByLabelText("Current password")).not.toBeInTheDocument();
  });

  it("accepts a one-character password", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("New password"), "a");
    await user.type(screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() => expect(changePassword).toHaveBeenCalledWith({ password: "a" }));
    expect(updateToken).toHaveBeenCalledWith("mv-user-rotated");
  });

  it("refuses a confirmation that differs", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("New password"), "first");
    await user.type(screen.getByLabelText("Confirm new password"), "second");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    expect(
      await screen.findByText("New password and confirmation do not match."),
    ).toBeInTheDocument();
    expect(changePassword).not.toHaveBeenCalled();
  });

  it("resets the password to none", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.click(screen.getByRole("button", { name: "Reset password" }));

    await waitFor(() => expect(changePassword).toHaveBeenCalledWith({ password: "" }));
    expect(
      await screen.findByText("Password reset. This account now has no password."),
    ).toBeInTheDocument();
  });

  it("offers no Reset password when the account must keep one", () => {
    render(<ChangePasswordSection canReset={false} />);

    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
  });

  it("asks the vault owner for the current password and sends it", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection canReset={false} requireCurrent />);

    await user.type(screen.getByLabelText("New password"), "keeperschoice");
    await user.type(screen.getByLabelText("Confirm new password"), "keeperschoice");
    // Nothing to send until the current password is typed.
    expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();

    await user.type(screen.getByLabelText("Current password"), "hunter2hunter2");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({
        password: "keeperschoice",
        current_password: "hunter2hunter2",
      }),
    );
    await waitFor(() => expect(screen.getByLabelText("Current password")).toHaveValue(""));
  });
});
