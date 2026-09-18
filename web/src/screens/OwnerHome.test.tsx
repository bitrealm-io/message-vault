/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VaultProviders } from "../test/vaultProviders";
import OwnerHome from "./OwnerHome";

const listAccounts = vi.hoisted(() => vi.fn());
const getVaultSettings = vi.hoisted(() => vi.fn());
const updateVaultSettings = vi.hoisted(() => vi.fn());
const updateAccount = vi.hoisted(() => vi.fn());
const setAccountPassword = vi.hoisted(() => vi.fn());
const createAccount = vi.hoisted(() => vi.fn());

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ logout: vi.fn(), updateToken: vi.fn(), accountId: 1 }),
}));

vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
  getVaultSettings: (...a: unknown[]) => getVaultSettings(...a),
  updateVaultSettings: (...a: unknown[]) => updateVaultSettings(...a),
  updateAccount: (...a: unknown[]) => updateAccount(...a),
  setAccountPassword: (...a: unknown[]) => setAccountPassword(...a),
  createAccount: (...a: unknown[]) => createAccount(...a),
}));

const anAccount = {
  account_id: 101,
  username: "bob",
  disabled: false,
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
  updateAccount.mockReset();
  setAccountPassword.mockReset();
  createAccount.mockReset();
  listAccounts.mockResolvedValue({ items: [anAccount] });
  getVaultSettings.mockResolvedValue({ public_registration: false });
  updateVaultSettings.mockResolvedValue({ public_registration: true });
  updateAccount.mockResolvedValue({ ...anAccount, disabled: true });
  setAccountPassword.mockResolvedValue(undefined);
  createAccount.mockResolvedValue({ ...anAccount, account_id: 102, username: "carol" });
});

afterEach(cleanup);

function renderHome(entries: string[] = ["/owner/accounts"]) {
  render(
    <VaultProviders>
      <MemoryRouter initialEntries={entries}>
        <Routes>
          <Route path="/owner/:section?" element={<OwnerHome />} />
        </Routes>
      </MemoryRouter>
    </VaultProviders>,
  );
}

function sectionLinks() {
  return within(screen.getByRole("navigation", { name: "Owner Home sections" })).getAllByRole(
    "button",
  );
}

function selectedSection(): string | undefined {
  return sectionLinks().find((b) => b.getAttribute("aria-current") === "page")?.textContent ?? "";
}

describe("OwnerHome", () => {
  it("offers exactly the four things the vault owner has, User Accounts first", () => {
    renderHome();

    expect(sectionLinks().map((b) => b.textContent)).toEqual([
      "User Accounts",
      "Vault",
      "Password",
      "Appearance",
    ]);
  });

  it("has no message-browsing chrome at all", () => {
    renderHome();

    // The owner holds no messages, so nothing that frames messages belongs here.
    expect(screen.queryByRole("combobox", { name: "Search messages" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Tags" })).not.toBeInTheDocument();
    expect(screen.queryByText("Conversations")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Export" })).not.toBeInTheDocument();
  });

  it("says what the vault owner is, and is not", () => {
    renderHome();

    expect(screen.getByText(/you read no messages/i)).toBeInTheDocument();
  });

  it("lists the accounts with counts and no message content", async () => {
    renderHome();

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
    renderHome();

    await screen.findByText("bob");
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).not.toContain("Admin");
  });

  it("sets an account's status from the dropdown, with no separate button", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await screen.findByText("bob");
    expect(screen.queryByRole("button", { name: "Disable" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Enable" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Status of bob/ }));
    await user.click(await screen.findByRole("option", { name: "Disabled" }));

    await waitFor(() => expect(updateAccount).toHaveBeenCalledWith(101, { disabled: true }));
  });

  it("resets a password only once it is typed twice the same way", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await screen.findByText("bob");
    await user.click(screen.getByRole("button", { name: "Reset password" }));
    const dialog = await screen.findByRole("dialog", { name: "Reset password" });
    const save = within(dialog).getByRole("button", { name: "Save" });

    await user.type(within(dialog).getByLabelText("New password"), "correct horse");
    await user.type(within(dialog).getByLabelText("Confirm password"), "correct hors");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Passwords do not match.");
    expect(save).toBeDisabled();

    await user.type(within(dialog).getByLabelText("Confirm password"), "e");
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
    expect(save).toBeEnabled();

    await user.click(save);
    await waitFor(() =>
      expect(setAccountPassword).toHaveBeenCalledWith(101, { password: "correct horse" }),
    );
    // Nothing about a forced change: the person keeps this password.
    expect(screen.queryByText(/made to replace/)).not.toBeInTheDocument();
  });

  it("adds an account only once its password is typed twice the same way", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await screen.findByText("bob");
    await user.click(screen.getByRole("button", { name: "Add account" }));
    const save = screen.getByRole("button", { name: "Save" });

    await user.type(screen.getByLabelText("New account's username"), "carol");
    await user.type(screen.getByLabelText("New account's password"), "hunter2hunter2");
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText("Confirm the new account's password"), "hunter2hunter2");
    expect(save).toBeEnabled();

    await user.click(save);
    await waitFor(() =>
      expect(createAccount).toHaveBeenCalledWith({ username: "carol", password: "hunter2hunter2" }),
    );
  });

  it("opens the section named in the address", async () => {
    renderHome(["/owner/vault"]);

    expect(selectedSection()).toBe("Vault");
    expect(
      await screen.findByText(/Let anyone reaching this vault create their own account/),
    ).toBeInTheDocument();
  });

  it("lands /owner and an unknown section on User Accounts", async () => {
    renderHome(["/owner"]);
    expect(selectedSection()).toBe("User Accounts");
    cleanup();

    renderHome(["/owner/conversations"]);
    expect(selectedSection()).toBe("User Accounts");
    expect(await screen.findByText("bob")).toBeInTheDocument();
  });

  it("moves between sections from the side panel", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await user.click(screen.getByRole("button", { name: "Password" }));
    expect(selectedSection()).toBe("Password");
    expect(await screen.findByText("Change Password")).toBeInTheDocument();
  });

  it("turns public registration on from the Vault section", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/vault"]);

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
    renderHome(["/owner/password"]);

    expect(await screen.findByText("Change Password")).toBeInTheDocument();
    // No profile, no time zone, no API tokens, no danger zone: the owner has
    // no vault for any of them to act on.
    expect(screen.queryByText("Username")).not.toBeInTheDocument();
    expect(screen.queryByText(/API tokens/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Time Zone/i)).not.toBeInTheDocument();
  });
});
