/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VaultProviders } from "../test/vaultProviders";
import OwnerConsole from "./OwnerConsole";

const listAccounts = vi.hoisted(() => vi.fn());
const getVaultSettings = vi.hoisted(() => vi.fn());
const updateVaultSettings = vi.hoisted(() => vi.fn());

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ logout: vi.fn(), updateToken: vi.fn(), accountId: 1 }),
}));

vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
  getVaultSettings: (...a: unknown[]) => getVaultSettings(...a),
  updateVaultSettings: (...a: unknown[]) => updateVaultSettings(...a),
}));

const anAccount = {
  account_id: 101,
  username: "bob",
  disabled: false,
  must_change_password: false,
  can_import: true,
  can_export: true,
  can_delete: false,
  message_count: 1234,
  storage_bytes: 2048,
};

beforeEach(() => {
  listAccounts.mockReset();
  getVaultSettings.mockReset();
  updateVaultSettings.mockReset();
  listAccounts.mockResolvedValue({ items: [anAccount] });
  getVaultSettings.mockResolvedValue({ public_registration: false });
  updateVaultSettings.mockResolvedValue({ public_registration: true });
});

afterEach(cleanup);

function renderConsole(entries: string[] = ["/admin"]) {
  render(
    <VaultProviders>
      <MemoryRouter initialEntries={entries}>
        <OwnerConsole />
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("OwnerConsole", () => {
  it("offers exactly the four things the vault owner has", () => {
    renderConsole();

    const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
    expect(tabs).toEqual(["User Accounts", "Vault", "Password", "Appearance"]);
  });

  it("has no message-browsing chrome at all", () => {
    renderConsole();

    // The owner holds no messages, so nothing that frames messages belongs here.
    expect(screen.queryByRole("combobox", { name: "Search messages" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Tags" })).not.toBeInTheDocument();
    expect(screen.queryByText("Conversations")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Import" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Export" })).not.toBeInTheDocument();
  });

  it("says what the vault owner is, and is not", () => {
    renderConsole();

    expect(screen.getByText(/you read no messages/i)).toBeInTheDocument();
  });

  it("lists the accounts with counts and no message content", async () => {
    renderConsole();

    expect(await screen.findByText("bob")).toBeInTheDocument();
    expect(screen.getByText("1,234")).toBeInTheDocument();
    // Column headers are metadata only.
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).toEqual([
      "Account",
      "Status",
      "Messages",
      "Storage",
      "Import",
      "Export",
      "Delete",
      "Actions",
    ]);
  });

  it("has no Admin column, because no account can be made one", async () => {
    renderConsole();

    await screen.findByText("bob");
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).not.toContain("Admin");
  });

  it("marks an account that has not replaced the password the owner chose", async () => {
    listAccounts.mockResolvedValue({
      items: [{ ...anAccount, must_change_password: true }],
    });
    renderConsole();

    expect(await screen.findByText("(has not set a password)")).toBeInTheDocument();
  });

  it("opens the tab named in the query string", async () => {
    renderConsole(["/admin?tab=vault"]);

    expect(screen.getByRole("tab", { name: "Vault" })).toHaveAttribute("aria-selected", "true");
    expect(
      await screen.findByText(/Let anyone reaching this vault create their own account/),
    ).toBeInTheDocument();
  });

  it("falls an unknown tab back to User Accounts", () => {
    renderConsole(["/admin?tab=conversations"]);

    expect(screen.getByRole("tab", { name: "User Accounts" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("turns public registration on from the Vault tab", async () => {
    const user = userEvent.setup({ delay: null });
    renderConsole(["/admin?tab=vault"]);

    const box = await screen.findByRole("checkbox", {
      name: /Let anyone reaching this vault create their own account/,
    });
    expect(box).not.toBeChecked();

    await user.click(box);

    await waitFor(() =>
      expect(updateVaultSettings).toHaveBeenCalledWith({
        public_registration: true,
      }),
    );
  });

  it("offers the owner a password and nothing else of their own", async () => {
    renderConsole(["/admin?tab=password"]);

    expect(await screen.findByText("Change Password")).toBeInTheDocument();
    // No profile, no time zone, no API tokens, no danger zone: the owner has
    // no vault for any of them to act on.
    expect(screen.queryByText("Username")).not.toBeInTheDocument();
    expect(screen.queryByText(/API tokens/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Time Zone/i)).not.toBeInTheDocument();
  });
});
