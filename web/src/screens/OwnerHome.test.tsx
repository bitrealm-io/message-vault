/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider } from "../lib/ThemeProvider";
import { VaultProviders } from "../test/vaultProviders";
import OwnerHome from "./OwnerHome";

const listAccounts = vi.hoisted(() => vi.fn());
const getVaultSettings = vi.hoisted(() => vi.fn());
const updateVaultSettings = vi.hoisted(() => vi.fn());
const updateAccount = vi.hoisted(() => vi.fn());
const setAccountPassword = vi.hoisted(() => vi.fn());
const createAccount = vi.hoisted(() => vi.fn());
const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccount = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const deleteAccountById = vi.hoisted(() => vi.fn());
const deleteAccountMessages = vi.hoisted(() => vi.fn());

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
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccount: (...a: unknown[]) => getAccount(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  deleteAccountById: (...a: unknown[]) => deleteAccountById(...a),
  deleteAccountMessages: (...a: unknown[]) => deleteAccountMessages(...a),
}));

const anAccount = {
  account_id: 101,
  username: "bob",
  preferred_name: "Bob Archer",
  time_zone: "America/New_York",
  phones: ["+15555550100"],
  emails: [],
  is_demo: false,
  is_owner: false,
  disabled: false,
  can_import: true,
  can_export: true,
  can_delete: false,
  message_count: 1234,
  storage_bytes: 2048,
  last_login_at: null,
};

/** The vault owner's own row, which leads the list and is account 1, the one logged in. */
const theOwner = {
  ...anAccount,
  account_id: 1,
  username: "root",
  preferred_name: null,
  phones: [],
  is_owner: true,
  message_count: 0,
  storage_bytes: 0,
};

beforeEach(() => {
  // jsdom has no matchMedia, and the theme reads the system colour scheme from it.
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  listAccounts.mockReset();
  getVaultSettings.mockReset();
  updateVaultSettings.mockReset();
  updateAccount.mockReset();
  setAccountPassword.mockReset();
  createAccount.mockReset();
  getAccountProfile.mockReset();
  getAccount.mockReset();
  getAccountStorage.mockReset();
  deleteAccountById.mockReset();
  deleteAccountMessages.mockReset();
  getAccountProfile.mockResolvedValue(theOwner);
  getAccount.mockResolvedValue(anAccount);
  getAccountStorage.mockResolvedValue({ total_bytes: 2048, attachment_count: 7 });
  deleteAccountById.mockResolvedValue(undefined);
  deleteAccountMessages.mockResolvedValue(undefined);
  listAccounts.mockResolvedValue({ items: [theOwner, anAccount] });
  getVaultSettings.mockResolvedValue({ public_registration: false });
  updateVaultSettings.mockResolvedValue({ public_registration: true });
  updateAccount.mockResolvedValue({ ...anAccount, disabled: true });
  setAccountPassword.mockResolvedValue(undefined);
  createAccount.mockResolvedValue({ ...anAccount, account_id: 102, username: "carol" });
});

afterEach(cleanup);

function renderHome(entries: string[] = ["/owner/accounts"]) {
  render(
    <ThemeProvider>
      <VaultProviders>
        <MemoryRouter initialEntries={entries}>
          <Routes>
            <Route path="/owner/:section?/:accountId?" element={<OwnerHome />} />
          </Routes>
        </MemoryRouter>
      </VaultProviders>
    </ThemeProvider>,
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
  it("lists Vault Settings, then User Accounts, and nothing else in the side panel", () => {
    renderHome();

    expect(sectionLinks().map((b) => b.textContent)).toEqual(["Vault Settings", "User Accounts"]);
  });

  it("has the header every account sees: the product name, a search bar, the account button", () => {
    renderHome();

    expect(screen.getByText("Message Vault")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Search accounts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Account menu" })).toBeInTheDocument();
  });

  it("has nothing that frames messages", () => {
    renderHome();

    // The owner holds no messages, so nothing that frames messages belongs here.
    expect(screen.queryByRole("combobox", { name: "Search messages" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Tags" })).not.toBeInTheDocument();
    expect(screen.queryByText("Conversations")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Export" })).not.toBeInTheDocument();
  });

  it("narrows the accounts table to the usernames the search bar matches", async () => {
    const user = userEvent.setup({ delay: null });
    listAccounts.mockResolvedValue({
      items: [anAccount, { ...anAccount, account_id: 102, username: "carol" }],
    });
    renderHome();

    await screen.findByText("bob");
    await user.type(screen.getByRole("combobox", { name: "Search accounts" }), "CAR");

    expect(screen.getByText("carol")).toBeInTheDocument();
    expect(screen.queryByText("bob")).not.toBeInTheDocument();
    // No search words and no advanced form: a username is all there is to match.
    expect(screen.queryByRole("option", { name: "Advanced search" })).not.toBeInTheDocument();
  });

  it("searches accounts from another section by going to User Accounts", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/vault"]);

    await user.type(screen.getByRole("combobox", { name: "Search accounts" }), "b");

    expect(selectedSection()).toBe("User Accounts");
    expect(await screen.findByText("bob")).toBeInTheDocument();
  });

  it("opens the owner's Settings from the account button", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await screen.findByText("bob");
    await user.click(screen.getByRole("button", { name: "Account menu" }));
    await user.click(await screen.findByRole("menuitem", { name: "Settings" }));

    // The owner's own row in User Accounts is the owner's Settings.
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(await screen.findByText("Change Password")).toBeInTheDocument();
    expect(selectedSection()).toBe("User Accounts");
  });

  it("lists the vault owner first, with no status or permissions to set", async () => {
    renderHome();

    const rows = (await screen.findAllByRole("row")).slice(1);
    expect(within(rows[0]).getByRole("button", { name: "Settings for root" })).toBeInTheDocument();
    expect(within(rows[0]).getByText("Vault owner")).toBeInTheDocument();
    // The owner cannot be disabled and holds no messages to import, export or delete.
    expect(within(rows[0]).queryByRole("button", { name: /Status of/ })).not.toBeInTheDocument();
    expect(within(rows[0]).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(within(rows[1]).getByRole("button", { name: "Settings for bob" })).toBeInTheDocument();
  });

  it("shows an account's preferred name under its username, and searches it too", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    expect(await screen.findByText("Bob Archer")).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search accounts" }), "archer");

    expect(screen.getByText("bob")).toBeInTheDocument();
    expect(screen.queryByText("root")).not.toBeInTheDocument();
  });

  it("opens an account's Settings from its name, with the account's own tabs", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    await user.click(await screen.findByRole("button", { name: "Settings for bob" }));

    expect(await screen.findByRole("heading", { name: "Settings for bob" })).toBeInTheDocument();
    expect(getAccount).toHaveBeenCalledWith(101, expect.anything());
    // System, Convert and Appearance are this device's, not bob's.
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Account",
      "Profile",
      "Storage",
    ]);
    // API tokens are the account holder's own to see.
    expect(screen.queryByText(/API tokens/i)).not.toBeInTheDocument();
  });

  it("shows an account's profile without offering to change it", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Profile" }));

    expect(await screen.findByLabelText("Display name")).toHaveValue("Bob Archer");
    expect(screen.getByLabelText("Display name")).toHaveAttribute("readonly");
    expect(screen.getByLabelText("Time zone")).toHaveValue("America/New_York");
    expect(screen.getByText("+15555550100")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
  });

  it("shows how much an account holds, and nothing of what", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Storage" }));

    await waitFor(() => expect(getAccountStorage).toHaveBeenCalledWith(expect.anything(), 101));
    expect(screen.queryByText(/import history/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/largest/i)).not.toBeInTheDocument();
  });

  it("deletes an account from its Settings and returns to User Accounts", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete bob's account?" });
    // The owner deletes on the strength of the count, so the dialog states it.
    expect(dialog).toHaveTextContent("1,234 messages");
    await user.click(within(dialog).getByRole("button", { name: "Delete account" }));

    await waitFor(() => expect(deleteAccountById).toHaveBeenCalledWith(101));
    expect(await screen.findByRole("heading", { name: "User Accounts" })).toBeInTheDocument();
  });

  it("deletes an account's messages from its Settings", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete all messages" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete bob's messages?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete all messages" }));

    await waitFor(() => expect(deleteAccountMessages).toHaveBeenCalledWith(101));
  });

  it("lists the accounts with counts and no message content", async () => {
    renderHome();

    expect(await screen.findByText("bob")).toBeInTheDocument();
    expect(screen.getByText("1,234")).toBeInTheDocument();
    // Column headers are metadata only.
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).toEqual(["Account", "Status", "Last login", "Messages", "Storage"]);
    // The table sets nothing: status reads as text, and the permissions, like
    // what was the Actions column, are in the account's Settings, behind its name.
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete account" })).not.toBeInTheDocument();
  });

  it("has no Admin column, because no account can be made one", async () => {
    renderHome();

    await screen.findByText("bob");
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).not.toContain("Admin");
  });

  it("shows when each account last logged in, or Never", async () => {
    listAccounts.mockResolvedValue({
      items: [
        anAccount,
        {
          ...anAccount,
          account_id: 102,
          username: "carol",
          last_login_at: "2026-09-17T14:05:00Z",
        },
      ],
    });
    renderHome();

    expect(await screen.findByText("Never")).toBeInTheDocument();
    // Rendered in the browser's own zone and locale, so match the parts
    // that survive either way.
    expect(screen.getByText(/2026/)).toBeInTheDocument();
  });

  it("sets an account's status from its Settings", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("button", { name: /Status/ }));
    await user.click(await screen.findByRole("option", { name: "Disabled" }));

    await waitFor(() => expect(updateAccount).toHaveBeenCalledWith(101, { disabled: true }));
  });

  it("sets an account's permissions from its Settings, under Permissions", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    expect(await screen.findByRole("heading", { name: "Permissions" })).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: "Delete messages and attachments" }));

    await waitFor(() => expect(updateAccount).toHaveBeenCalledWith(101, { can_delete: true }));
  });

  it("gives the owner's own account no status and no permissions", async () => {
    renderHome(["/owner/accounts/1"]);

    await screen.findByRole("heading", { name: "Change Password" });
    expect(screen.queryByRole("heading", { name: "Permissions" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Status" })).not.toBeInTheDocument();
  });

  it("sets an account's password from its Settings, typed twice the same way", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.type(await screen.findByLabelText("New password"), "correct horse");
    await user.type(screen.getByLabelText("Confirm new password"), "correct hors");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(await screen.findByText(/do not match/)).toBeInTheDocument();
    expect(setAccountPassword).not.toHaveBeenCalled();

    await user.type(screen.getByLabelText("Confirm new password"), "e");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() =>
      expect(setAccountPassword).toHaveBeenCalledWith(101, { password: "correct horse" }),
    );
    // Nothing about a forced change: the person keeps this password.
    expect(screen.queryByText(/made to replace/)).not.toBeInTheDocument();
  });

  it("clears a user's password from the account's Settings", async () => {
    const user = userEvent.setup();
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("button", { name: "Reset password" }));

    await waitFor(() => expect(setAccountPassword).toHaveBeenCalledWith(101, { password: "" }));
  });

  it("offers the owner no way to reset their own password to none", async () => {
    renderHome(["/owner/accounts/1"]);

    expect(await screen.findByRole("button", { name: "Change password" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
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

    expect(selectedSection()).toBe("Vault Settings");
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

    await user.click(screen.getByRole("button", { name: "Vault Settings" }));
    expect(selectedSection()).toBe("Vault Settings");
    expect(
      await screen.findByText(/Let anyone reaching this vault create their own account/),
    ).toBeInTheDocument();
  });

  it("turns public registration on from Vault Settings", async () => {
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

  it("gives the owner's own Settings the tabs that mean something to an owner", async () => {
    renderHome(["/owner/accounts/1"]);

    expect(await screen.findByText("Change Password")).toBeInTheDocument();
    // The owner's own account is read as the logged-in account, not as a managed one.
    expect(getAccount).not.toHaveBeenCalled();
    // No Storage, System or Convert: the owner holds no messages.
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Account",
      "Profile",
      "Appearance",
    ]);
    // The owner's account reaches every other, so its password change asks for the current one.
    expect(screen.getByLabelText("Current password")).toBeInTheDocument();
    // No API tokens and no danger zone: the owner mints no token and cannot be deleted.
    expect(screen.queryByText(/API tokens/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Danger zone/ })).not.toBeInTheDocument();
  });

  it("shows the owner a name and a time zone on Profile, and no handles", async () => {
    renderHome(["/owner/accounts/1?tab=profile"]);

    expect(await screen.findByText("Display Name")).toBeInTheDocument();
    expect(screen.getByText("Time Zone")).toBeInTheDocument();
    expect(screen.queryByText("My Handles")).not.toBeInTheDocument();
  });
});
