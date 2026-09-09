/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VaultProviders } from "../test/vaultProviders";
import SetPasswordScreen from "./SetPasswordScreen";

const updateToken = vi.fn();
const refreshProfile = vi.fn(async () => null);
const changePassword = vi.hoisted(() => vi.fn());

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ updateToken }),
}));

vi.mock("../lib/useAccountProfile", () => ({
  useFetchAccountProfile: () => refreshProfile,
}));

vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  changePassword: (...args: unknown[]) => changePassword(...args),
}));

beforeEach(() => {
  updateToken.mockReset();
  refreshProfile.mockClear();
  changePassword.mockReset();
  changePassword.mockResolvedValue({ token: "mv-user-fresh" });
});

afterEach(cleanup);

function renderScreen() {
  render(
    <VaultProviders>
      <SetPasswordScreen />
    </VaultProviders>,
  );
}

describe("SetPasswordScreen", () => {
  it("sends the current and new password, then swaps in the rotated session", async () => {
    const user = userEvent.setup({ delay: null });
    renderScreen();

    await user.type(screen.getByLabelText("Current Password"), "ownerspick1");
    await user.type(screen.getByLabelText("New Password"), "myownchoice");
    await user.type(screen.getByLabelText("Confirm New Password"), "myownchoice");
    await user.click(screen.getByRole("button", { name: "Set Password" }));

    await waitFor(() => {
      expect(changePassword).toHaveBeenCalledWith({
        current_password: "ownerspick1",
        password: "myownchoice",
      });
    });
    // Changing the password rotates the session, so the old token is dead.
    expect(updateToken).toHaveBeenCalledWith("mv-user-fresh");
    // And the profile is re-read, because clearing must_change_password is
    // what lets the guard stop sending this account back here.
    expect(refreshProfile).toHaveBeenCalledWith(true);
  });

  it("refuses a mismatch without calling the vault", async () => {
    const user = userEvent.setup({ delay: null });
    renderScreen();

    await user.type(screen.getByLabelText("Current Password"), "ownerspick1");
    await user.type(screen.getByLabelText("New Password"), "myownchoice");
    await user.type(screen.getByLabelText("Confirm New Password"), "typedwrong");
    await user.click(screen.getByRole("button", { name: "Set Password" }));

    expect(await screen.findByText("Passwords do not match.")).toBeInTheDocument();
    expect(changePassword).not.toHaveBeenCalled();
  });

  it("says why the account is here", () => {
    renderScreen();

    expect(screen.getByText(/created with a password the vault owner chose/)).toBeInTheDocument();
  });

  it("leaves the length rule to the vault", async () => {
    const user = userEvent.setup({ delay: null });
    changePassword.mockRejectedValue(new Error("password must be at least 8 characters"));
    renderScreen();

    await user.type(screen.getByLabelText("Current Password"), "ownerspick1");
    await user.type(screen.getByLabelText("New Password"), "short");
    await user.type(screen.getByLabelText("Confirm New Password"), "short");
    await user.click(screen.getByRole("button", { name: "Set Password" }));

    // The form sent it rather than second-guessing the server's rule, and
    // shows back what the vault said (sentence-cased by the error footer).
    await waitFor(() => expect(changePassword).toHaveBeenCalled());
    expect(await screen.findByText("Password must be at least 8 characters")).toBeInTheDocument();
  });
});
