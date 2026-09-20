/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { APP_BUILD } from "../lib/build";
import { productVersionOf } from "../lib/buildFormat";
import { ThemeProvider } from "../lib/ThemeProvider";
import { VaultProviders } from "../test/vaultProviders";
import OwnerHome from "./OwnerHome";

const listAccounts = vi.hoisted(() => vi.fn());
const getVaultSettings = vi.hoisted(() => vi.fn());
const getVaultState = vi.hoisted(() => vi.fn());
const updateVaultSettings = vi.hoisted(() => vi.fn());
const updateAccount = vi.hoisted(() => vi.fn());
const setAccountPassword = vi.hoisted(() => vi.fn());
const createAccount = vi.hoisted(() => vi.fn());
const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccount = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const getAccountImport = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());
const getImportContacts = vi.hoisted(() => vi.fn());
const deleteAccountById = vi.hoisted(() => vi.fn());
const deleteAccountMessages = vi.hoisted(() => vi.fn());

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ logout: vi.fn(), updateToken: vi.fn(), accountId: 1 }),
}));

vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
  getVaultSettings: (...a: unknown[]) => getVaultSettings(...a),
  getVaultState: (...a: unknown[]) => getVaultState(...a),
  updateVaultSettings: (...a: unknown[]) => updateVaultSettings(...a),
  updateAccount: (...a: unknown[]) => updateAccount(...a),
  setAccountPassword: (...a: unknown[]) => setAccountPassword(...a),
  createAccount: (...a: unknown[]) => createAccount(...a),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccount: (...a: unknown[]) => getAccount(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  listAccountImports: (...a: unknown[]) => listAccountImports(...a),
  getAccountImport: (...a: unknown[]) => getAccountImport(...a),
  listAccountExports: (...a: unknown[]) => listAccountExports(...a),
  getImportContacts: (...a: unknown[]) => getImportContacts(...a),
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
  app: null,
  app_version: null,
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
  getVaultState.mockReset();
  updateVaultSettings.mockReset();
  updateAccount.mockReset();
  setAccountPassword.mockReset();
  createAccount.mockReset();
  getAccountProfile.mockReset();
  getAccount.mockReset();
  getAccountStorage.mockReset();
  listAccountImports.mockReset();
  getAccountImport.mockReset();
  listAccountExports.mockReset();
  getImportContacts.mockReset();
  deleteAccountById.mockReset();
  deleteAccountMessages.mockReset();
  getAccountProfile.mockResolvedValue(theOwner);
  getAccount.mockResolvedValue(anAccount);
  getAccountStorage.mockResolvedValue({
    total_bytes: 2048,
    attachment_count: 7,
    top_attachments: [],
  });
  listAccountImports.mockResolvedValue({ items: [anImport], total: 1, limit: 40, offset: 0 });
  getAccountImport.mockResolvedValue(anImportDetail);
  listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 });
  deleteAccountById.mockResolvedValue(undefined);
  deleteAccountMessages.mockResolvedValue(undefined);
  listAccounts.mockResolvedValue({ items: [theOwner, anAccount] });
  getVaultSettings.mockResolvedValue({ public_registration: false });
  // The vault and this app are the same release unless a test says otherwise.
  getVaultState.mockResolvedValue({
    state: "closed",
    version: APP_BUILD,
    schema_fingerprint: 1234567890,
  });
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

/** One Import Run of bob's, as the list answers it. */
const anImport = {
  id: 9,
  source: "imessage",
  status: "completed",
  started_at: "2026-09-01T10:00:00Z",
  finished_at: "2026-09-01T10:05:00Z",
  message_count: 1234,
  attachment_count: 7,
  bytes_uploaded: 2048,
};

/** The same run in full, which is what opening its row reads. */
const anImportDetail = {
  ...anImport,
  tool: "desktop",
  mode: "full",
  stage: "done",
  duration_ms: 300000,
  parse_ms: null,
  attachments_ms: null,
  prepare_ms: null,
  upload_ms: null,
  summary: {},
  issues: [],
  contacts_new: 12,
  contacts_changed: 3,
};

describe("OwnerHome", () => {
  it("lists Dashboard, Settings, User Accounts, Activity and Logs in the side panel", () => {
    renderHome();

    expect(sectionLinks().map((b) => b.textContent)).toEqual([
      "Dashboard",
      "Settings",
      "User Accounts",
      "Activity",
      "Logs",
    ]);
  });

  it.each([
    ["dashboard", "Dashboard"],
    ["activity", "Activity"],
    ["logs", "Logs"],
  ])("opens /owner/%s on its name and loads nothing", (id, label) => {
    renderHome([`/owner/${id}`]);

    expect(selectedSection()).toBe(label);
    expect(screen.getByRole("heading", { name: label })).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(listAccounts).not.toHaveBeenCalled();
    expect(getVaultSettings).not.toHaveBeenCalled();
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
    renderHome(["/owner/settings"]);

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
    // The gear opens the account; the name itself is plain text.
    expect(within(rows[1]).getByText("bob").closest("button")).toBeNull();
  });

  it("shows an account's preferred name under its username, and searches it too", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome();

    expect(await screen.findByText("Bob Archer")).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search accounts" }), "archer");

    expect(screen.getByText("bob")).toBeInTheDocument();
    expect(screen.queryByText("root")).not.toBeInTheDocument();
  });

  it("opens an account's Settings from the gear in its row, with the account's own tabs", async () => {
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

  it("shows an account's last login and its app on Profile, marking another release", async () => {
    getVaultState.mockResolvedValue({
      state: "closed",
      version: "0.10.0+343fe0d8",
      schema_fingerprint: 1234567890,
    });
    getAccount.mockResolvedValue({
      ...anAccount,
      last_login_at: "2026-09-01T10:00:00Z",
      app: "desktop",
      app_version: "0.9.0+aaaa1111",
    });
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Profile" }));

    const lastLogin = await screen.findByRole("heading", { name: "Last Login" });
    expect(lastLogin.nextElementSibling).toHaveTextContent("2026");
    const app = screen.getByRole("heading", { name: "App" }).nextElementSibling as HTMLElement;
    expect(app).toHaveTextContent("Desktop app 0.9.0+aaaa1111");
    await waitFor(() => expect(app).toHaveTextContent("This vault is 0.10.0"));
  });

  it("says Never and not connected on Profile for an account that has done neither", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Profile" }));

    const lastLogin = await screen.findByRole("heading", { name: "Last Login" });
    expect(lastLogin.nextElementSibling).toHaveTextContent("Never");
    expect(screen.getByText("Has not connected yet.")).toBeInTheDocument();
  });

  it("shows the owner an account's Storage as the account sees it", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Storage" }));

    await waitFor(() => expect(getAccountStorage).toHaveBeenCalledWith(expect.anything(), 101));
    expect(listAccountImports).toHaveBeenCalledWith(expect.anything(), 101);
    expect(listAccountExports).toHaveBeenCalledWith(expect.anything(), 101);
    // What the accounts table used to carry: the message count and the storage total.
    expect(await screen.findByText(/1,234 messages/)).toBeInTheDocument();
    expect(screen.getByText(/7 attachments/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Import history" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /export history/i })).toBeInTheDocument();
  });

  it("lists an account's largest attachments for the owner by name and size, with no conversation", async () => {
    getAccountStorage.mockResolvedValue({
      total_bytes: 3000,
      attachment_count: 1,
      // What the vault answers the owner: the file, and not where it sits.
      top_attachments: [
        { id: 5, original_name: "big.mov", mime_type: "video/quicktime", size_bytes: 3000 },
      ],
    });
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Storage" }));

    expect(await screen.findByText("big.mov")).toBeInTheDocument();
    const table = screen.getByText("big.mov").closest("table") as HTMLElement;
    const headers = Array.from(table.querySelectorAll("th")).map((h) => h.textContent);
    expect(headers).toEqual(["Name", "Size"]);
  });

  it("counts the contacts an import made for the owner, and does not name them", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/accounts/101"]);

    await user.click(await screen.findByRole("tab", { name: "Storage" }));
    await user.click(await screen.findByText("imessage"));

    await waitFor(() => expect(getAccountImport).toHaveBeenCalledWith(9, expect.anything(), 101));
    expect(await screen.findByText("12 new, 3 changed")).toBeInTheDocument();
    expect(getImportContacts).not.toHaveBeenCalled();
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

  it("lists each user with a status and a last login, and nothing else", async () => {
    renderHome();

    expect(await screen.findByText("bob")).toBeInTheDocument();
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).toEqual(["User", "Status", "Last login"]);
    // What an account holds is under its Storage tab, and its app under Profile.
    expect(screen.queryByText("1,234")).not.toBeInTheDocument();
    expect(screen.queryByText(/The accounts on this vault/)).not.toBeInTheDocument();
    // The table sets nothing: status reads as text, and the permissions, like
    // what was the Actions column, are in the account's Settings, behind its gear.
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

  it("states the vault's version and schema fingerprint in Settings", async () => {
    renderHome(["/owner/settings"]);

    const version = await screen.findByText("Version");
    expect(version.nextElementSibling).toHaveTextContent(APP_BUILD);
    expect(screen.getByText("Schema fingerprint").nextElementSibling).toHaveTextContent(
      "1234567890",
    );
  });

  it("says nothing under the header while the vault and the app are one release", async () => {
    renderHome();

    await screen.findByText("bob");
    await waitFor(() => expect(getVaultState).toHaveBeenCalled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("states both versions under the header when the vault is another release", async () => {
    getVaultState.mockResolvedValue({
      state: "closed",
      version: "0.10.0",
      schema_fingerprint: 1234567890,
    });
    renderHome();

    expect(await screen.findByRole("status")).toHaveTextContent(
      `This vault is 0.10.0. This app is ${productVersionOf(APP_BUILD)}.`,
    );
    // It blocks nothing: the screen under it still loads and works.
    expect(await screen.findByText("bob")).toBeInTheDocument();
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
    renderHome(["/owner/settings"]);

    expect(selectedSection()).toBe("Settings");
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

    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(selectedSection()).toBe("Settings");
    expect(
      await screen.findByText(/Let anyone reaching this vault create their own account/),
    ).toBeInTheDocument();
  });

  it("turns public registration on from Settings", async () => {
    const user = userEvent.setup({ delay: null });
    renderHome(["/owner/settings"]);

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
    // The owner's own account is read as the signed-in account, not as a managed one.
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
