/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, VaultProviders } from "../test/vaultProviders";
import SettingsScreen from "./SettingsScreen";

/**
 * Settings holds no account management. The vault owner manages accounts from
 * a console of their own, and an ordinary account never could. `?tab=users`
 * must therefore fall back to Account rather than render anything.
 */

const profileState = vi.hoisted(() => ({
  profile: null as { username: string } | null,
}));

const tauriState = vi.hoisted(() => ({ isTauri: false }));

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: profileState.profile }),
}));

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ ...mockedAuth, updateToken: vi.fn() }),
}));

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  tauriState.isTauri = false;
});

function baseProfile() {
  return {
    account_id: 7,
    username: "bob",
    preferred_name: null,
    phones: [],
    emails: [],
    can_import: true,
    can_export: true,
    can_delete: false,
  };
}

function renderSettings(initialEntries: string[]) {
  return render(
    <VaultProviders>
      <MemoryRouter initialEntries={initialEntries}>
        <SettingsScreen />
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("SettingsScreen has no account management", () => {
  it("offers no Users tab", () => {
    profileState.profile = baseProfile();
    renderSettings(["/settings"]);

    expect(screen.queryByRole("tab", { name: "Users" })).not.toBeInTheDocument();
  });

  it("falls ?tab=users back to the account tab", () => {
    profileState.profile = baseProfile();
    renderSettings(["/settings?tab=users"]);

    expect(screen.queryByRole("tab", { name: "Users" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
  });
});

/**
 * Convert runs `message-reexport` inside the desktop process, so the tab is a
 * desktop-only tool. In a browser the tab must not exist and `?tab=convert`
 * must fall back to Account, the same way the admin gate treats Users.
 */
describe("SettingsScreen convert gate", () => {
  it("hides the Convert tab in the browser and falls ?tab=convert back to Account", () => {
    profileState.profile = baseProfile();
    renderSettings(["/settings?tab=convert"]);

    expect(screen.queryByRole("tab", { name: "Convert" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByText(/Manage your account, profile, storage, system, and appearance\./),
    ).toBeInTheDocument();
  });

  it("shows the Convert tab and tool in the desktop app", () => {
    tauriState.isTauri = true;
    profileState.profile = baseProfile();
    renderSettings(["/settings?tab=convert"]);

    expect(screen.getByRole("tab", { name: "Convert" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Input folder")).toBeInTheDocument();
    expect(screen.getByLabelText("Output folder")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Convert" })).toBeDisabled();
    expect(
      screen.getByText(/Manage your account, profile, storage, system, convert, and appearance\./),
    ).toBeInTheDocument();
  });
});
