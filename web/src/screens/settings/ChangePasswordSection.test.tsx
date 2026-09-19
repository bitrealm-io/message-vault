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
  it("accepts a one-character password", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("Current password"), "old");
    await user.type(screen.getByLabelText("New password"), "a");
    await user.type(screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({ current_password: "old", password: "a" }),
    );
    expect(updateToken).toHaveBeenCalledWith("mv-user-rotated");
  });

  it("lets an account with no password set one", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("New password"), "first");
    await user.type(screen.getByLabelText("Confirm new password"), "first");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({ current_password: "", password: "first" }),
    );
  });

  it("clears the password with the current one", async () => {
    const user = userEvent.setup();
    render(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("Current password"), "old");
    await user.click(screen.getByRole("button", { name: "Clear password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({ current_password: "old", password: "" }),
    );
    expect(await screen.findByText("Password cleared.")).toBeInTheDocument();
  });

  it("offers no Clear password when the account must keep one", () => {
    render(<ChangePasswordSection canClear={false} />);

    expect(screen.queryByRole("button", { name: "Clear password" })).not.toBeInTheDocument();
  });
});
