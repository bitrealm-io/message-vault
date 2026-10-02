/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { renderWithProviders, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { ChangePasswordSection } from "./ChangePasswordSection";

const changePassword = vi.hoisted(() => vi.fn());
const updateToken = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 101, updateToken }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  changePassword: (...a: unknown[]) => changePassword(...a),
}));

beforeEach(() => {
  changePassword.mockReset();
  updateToken.mockReset();
  changePassword.mockResolvedValue({ token: "mc-user-rotated" });
});

afterEach(cleanup);

describe("ChangePasswordSection", () => {
  it("asks for the new password twice and never the current one", () => {
    renderWithProviders(<ChangePasswordSection />);

    expect(screen.getByLabelText("New password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm new password")).toBeInTheDocument();
    expect(screen.queryByLabelText("Current password")).not.toBeInTheDocument();
  });

  it("accepts a one-character password", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("New password"), "a");
    await user.type(screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({ password: "a", password_confirmation: "a" }),
    );
    expect(updateToken).toHaveBeenCalledWith("mc-user-rotated");
  });

  it("sends a differing confirmation to the server and shows its sentence", async () => {
    // The server checks the pair after the current password, so the screen
    // never judges it: the order of what a user hears is the server's.
    changePassword.mockRejectedValue(new Error("New passwords do not match."));
    const user = userEvent.setup();
    renderWithProviders(<ChangePasswordSection />);

    await user.type(screen.getByLabelText("New password"), "first");
    await user.type(screen.getByLabelText("Confirm new password"), "second");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    expect(await screen.findByText("New passwords do not match.")).toBeInTheDocument();
    expect(changePassword).toHaveBeenCalledWith({
      password: "first",
      password_confirmation: "second",
    });
    expect(updateToken).not.toHaveBeenCalled();
  });

  it("resets the password to none", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ChangePasswordSection />);

    await user.click(screen.getByRole("button", { name: "Reset password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({ password: "", password_confirmation: "" }),
    );
    expect(
      await screen.findByText(
        "Password reset. This account now has no password, and its API Tokens were revoked.",
      ),
    ).toBeInTheDocument();
  });

  it("offers no Reset password when the account must keep one", () => {
    renderWithProviders(<ChangePasswordSection canReset={false} />);

    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
  });

  it("asks the owner for the current password and sends it", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ChangePasswordSection canReset={false} requireCurrent />);

    await user.type(screen.getByLabelText("New password"), "keeperschoice");
    await user.type(screen.getByLabelText("Confirm new password"), "keeperschoice");
    // Nothing to send until the current password is typed.
    expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();

    await user.type(screen.getByLabelText("Current password"), "hunter2hunter2");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(changePassword).toHaveBeenCalledWith({
        password: "keeperschoice",
        password_confirmation: "keeperschoice",
        current_password: "hunter2hunter2",
      }),
    );
    await waitFor(() => expect(screen.getByLabelText("Current password")).toHaveValue(""));
  });

  it("marks the API Tokens and the profile stale, and says the tokens were revoked", async () => {
    // The server deletes every API Token of the account when its password
    // changes, and the profile's has_password changes with it. The table kept
    // listing the revoked tokens, and Delete account asked for no password.
    const shown = [keys.apiTokens.all, keys.accountProfile.all];
    const client = testQueryClient();
    seedEntries(client, 101, shown);
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={client}>
        <ChangePasswordSection />
      </QueryClientProvider>,
    );

    await user.type(screen.getByLabelText("New password"), "a");
    await user.type(screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    expect(
      await screen.findByText("Password changed. This account's API Tokens were revoked."),
    ).toBeInTheDocument();
    expect(freshEntries(client, 101, shown)).toEqual([]);
  });
});
