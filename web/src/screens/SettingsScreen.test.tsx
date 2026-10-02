/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getAccountProfile } from "../lib/serverApi";
import { mockedAuth, Providers } from "../test/providers";
import SettingsScreen from "./SettingsScreen";

/**
 * Settings holds no account management. The owner manages accounts from
 * a console of their own, and an ordinary account never could. `?tab=users`
 * must therefore fall back to Account rather than render anything.
 */

const tauriState = vi.hoisted(() => ({ isTauri: false }));

// The logged-in account's Settings read the account through
// `useSettingsAccount`, which calls this route function.
vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  getAccountProfile: vi.fn(),
}));

const getAccountProfileMock = vi.mocked(getAccountProfile);

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
  getAccountProfileMock.mockReset();
  getAccountProfileMock.mockResolvedValue(baseProfile());
});

function baseProfile(): Awaited<ReturnType<typeof getAccountProfile>> {
  return {
    account_id: 7,
    username: "bob",
    preferred_name: null,
    phones: [],
    emails: [],
    can_import: true,
    can_export: true,
    can_delete: false,
    disabled: false,
    has_password: true,
    is_demo: false,
    is_owner: false,
    message_count: 0,
    must_set_up_profile: false,
    storage_bytes: 0,
    time_zone: "UTC",
  };
}

function renderSettings(initialEntries: string[]) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={initialEntries}>
        <SettingsScreen />
      </MemoryRouter>
    </Providers>,
  );
}

describe("SettingsScreen has no account management", () => {
  it("offers no Users tab", () => {
    renderSettings(["/settings"]);

    expect(screen.queryByRole("tab", { name: "Users" })).not.toBeInTheDocument();
  });

  it("falls ?tab=users back to the account tab", () => {
    renderSettings(["/settings?tab=users"]);

    expect(screen.queryByRole("tab", { name: "Users" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
  });
});

/**
 * The owner holds no messages, so the owner's own Settings have no Storage
 * tab. The tabs follow the account the screen reads, so an ordinary
 * account keeps Storage.
 */
describe("SettingsScreen tabs follow the account it reads", () => {
  it("keeps Storage for an ordinary account", async () => {
    renderSettings(["/settings"]);

    expect(await screen.findByRole("tab", { name: "Storage" })).toBeInTheDocument();
  });

  it("drops Storage once the account read is the owner's", async () => {
    getAccountProfileMock.mockResolvedValue({ ...baseProfile(), is_owner: true });
    renderSettings(["/settings"]);

    // Storage shows until the account arrives, then goes.
    await vi.waitFor(() => {
      expect(screen.queryByRole("tab", { name: "Storage" })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("tab", { name: "Account" })).toBeInTheDocument();
    expect(getAccountProfileMock).toHaveBeenCalled();
  });
});

/**
 * Convert runs `message-reexport` inside the desktop process, so the tab is a
 * desktop-only tool. In a browser the tab must not exist and `?tab=convert`
 * must fall back to Account, like every other tab `visibleTabs` leaves out.
 */
describe("SettingsScreen convert gate", () => {
  it("hides the Convert tab in the browser and falls ?tab=convert back to Account", () => {
    renderSettings(["/settings?tab=convert"]);

    expect(screen.queryByRole("tab", { name: "Convert" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
  });

  it("shows the Convert tab and tool in the desktop app", () => {
    tauriState.isTauri = true;
    renderSettings(["/settings?tab=convert"]);

    expect(screen.getByRole("tab", { name: "Convert" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Input folder")).toBeInTheDocument();
    expect(screen.getByLabelText("Output folder")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Convert" })).toBeDisabled();
  });
});
