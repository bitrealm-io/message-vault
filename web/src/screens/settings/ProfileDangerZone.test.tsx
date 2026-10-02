/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProfileDangerZone } from "./ProfileDangerZone";

const deleteAccount = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ logout }),
}));

vi.mock("../owner/useOwnerAccounts", () => ({
  useDeleteAccount: () => ({ mutateAsync: vi.fn() }),
  useDeleteAccountMessages: () => ({ mutateAsync: vi.fn() }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  deleteAccount: (...a: unknown[]) => deleteAccount(...a),
}));

beforeEach(() => {
  deleteAccount.mockReset();
  logout.mockReset();
});

afterEach(cleanup);

describe("ProfileDangerZone", () => {
  it("shows the server's refusal inside the delete account dialog", async () => {
    deleteAccount.mockRejectedValue(new Error("Current password is incorrect."));
    const user = userEvent.setup({ delay: null });
    render(
      <MemoryRouter>
        <ProfileDangerZone isDemo={false} username="carol" hasPassword />
      </MemoryRouter>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");
    await user.type(within(dialog).getByLabelText("Current password"), "wrong");
    await user.click(within(dialog).getByRole("button", { name: "Permanently delete my account" }));

    expect(deleteAccount).toHaveBeenCalledWith({ confirm: true, current_password: "wrong" });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Current password is incorrect.",
    );
    expect(logout).not.toHaveBeenCalled();
  });
});
