/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import AppAccountMenu from "./AppAccountMenu";

const profileState = vi.hoisted(() => ({
  profile: null as { username: string; preferred_name?: string | null } | null,
}));
const authState = vi.hoisted(() => ({ logout: vi.fn(async () => {}) }));

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: profileState.profile, loading: false, error: "" }),
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ accountId: 7, token: "t", isAuthenticated: true, logout: authState.logout }),
}));

afterEach(() => {
  cleanup();
  authState.logout.mockClear();
});

function renderMenu() {
  return render(
    <MemoryRouter>
      <AppAccountMenu />
    </MemoryRouter>,
  );
}

describe("AppAccountMenu", () => {
  it("is a circle user button, not the app name", () => {
    profileState.profile = { username: "ada", preferred_name: "Ada Lovelace" };
    renderMenu();
    const trigger = screen.getByRole("button", { name: "Account menu" });
    expect(trigger.className).toContain("rounded-full");
    expect(trigger.textContent).toBe("");
    expect(screen.queryByText("Message Vault")).toBeNull();
  });

  it("shows the username and preferred name above Settings and Log out", async () => {
    const user = userEvent.setup();
    profileState.profile = { username: "ada", preferred_name: "Ada Lovelace" };
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Account menu" }));
    const menu = screen.getByRole("menu", { name: "Account menu" });
    expect(screen.getByTestId("account-menu-username").textContent).toBe("ada");
    expect(screen.getByTestId("account-menu-preferred-name").textContent).toBe("Ada Lovelace");
    const items = screen.getAllByRole("menuitem").map((el) => el.textContent);
    expect(items).toEqual(["Settings", "Log out"]);
    expect(menu.textContent).not.toContain("Sign out");
  });

  it("leaves out the preferred name line when none is set", async () => {
    const user = userEvent.setup();
    profileState.profile = { username: "ada", preferred_name: null };
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Account menu" }));
    expect(screen.getByTestId("account-menu-username").textContent).toBe("ada");
    expect(screen.queryByTestId("account-menu-preferred-name")).toBeNull();
  });

  it("logs out from the Log out item", async () => {
    const user = userEvent.setup();
    profileState.profile = { username: "ada" };
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Account menu" }));
    await user.click(screen.getByRole("menuitem", { name: "Log out" }));
    expect(authState.logout).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
