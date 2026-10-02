/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { ProfileDangerZone } from "./ProfileDangerZone";

const deleteAccount = vi.hoisted(() => vi.fn());
const deleteAllMessages = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 7, logout }),
}));

vi.mock("../owner/useOwnerAccounts", () => ({
  useDeleteAccount: () => ({ mutateAsync: vi.fn() }),
  useDeleteAccountMessages: () => ({ mutateAsync: vi.fn() }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  deleteAccount: (...a: unknown[]) => deleteAccount(...a),
  deleteAllMessages: (...a: unknown[]) => deleteAllMessages(...a),
}));

beforeEach(() => {
  deleteAccount.mockReset();
  deleteAllMessages.mockReset();
  logout.mockReset();
});

afterEach(cleanup);

describe("ProfileDangerZone", () => {
  it("shows the server's refusal inside the delete account dialog", async () => {
    deleteAccount.mockRejectedValue(new Error("Current password is incorrect."));
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone isDemo={false} username="carol" hasPassword />
        </MemoryRouter>
      </QueryClientProvider>,
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

  it("marks every screen that shows the account's messages stale once they are deleted", async () => {
    // Messages, contacts, the Trash, Storage and the counts on the profile all
    // showed the deleted messages from the cache until they were next fetched.
    const shown = [
      keys.conversations.all,
      keys.contacts.all,
      keys.trash.all,
      keys.storage.all,
      keys.accountProfile.all,
      keys.accountProfile.identities,
    ];
    deleteAllMessages.mockResolvedValue({ conversations: 3, attachments: 0 });
    const client = testQueryClient();
    seedEntries(client, 7, shown);
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProfileDangerZone isDemo={false} username="carol" hasPassword />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete all messages" }));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete all messages" }),
    );

    expect(deleteAllMessages).toHaveBeenCalledWith({ confirm: true });
    await waitFor(() => expect(freshEntries(client, 7, shown)).toEqual([]));
  });
});
