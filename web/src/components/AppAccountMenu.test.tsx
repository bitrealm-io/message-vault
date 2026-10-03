/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
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
    expect(screen.queryByText("Message Crate")).toBeNull();
  });

  it("shows the username beside the button without opening the menu", () => {
    profileState.profile = { username: "ada", preferred_name: "Ada Lovelace" };
    renderMenu();
    expect(screen.getByTestId("header-username").textContent).toBe("ada");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("cuts a long username short and keeps the whole of it in the tooltip", () => {
    const long = "a-very-long-username-that-would-push-the-search-bar";
    profileState.profile = { username: long };
    renderMenu();
    const name = screen.getByTestId("header-username");
    expect(name.className).toContain("truncate");
    expect(name.getAttribute("title")).toBe(long);
  });

  it("shows no username while the profile is loading", () => {
    profileState.profile = null;
    renderMenu();
    expect(screen.queryByTestId("header-username")).toBeNull();
  });

  it("shows the username and preferred name above Settings and Log out", async () => {
    const user = userEvent.setup();
    profileState.profile = { username: "ada", preferred_name: "Ada Lovelace" };
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Account menu" }));
    expect(screen.getByTestId("account-menu-username").textContent).toBe("ada");
    expect(screen.getByTestId("account-menu-preferred-name").textContent).toBe("Ada Lovelace");
    const items = screen.getAllByRole("menuitem").map((el) => el.textContent);
    expect(items).toEqual(["Settings", "Log out"]);
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

  it("opens from the keyboard and moves to an item by its first letter", async () => {
    const user = userEvent.setup();
    profileState.profile = { username: "ada" };
    renderMenu();

    act(() => screen.getByRole("button", { name: "Account menu" }).focus());
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Settings" })).toHaveFocus();

    await user.keyboard("l");
    expect(screen.getByRole("menuitem", { name: "Log out" })).toHaveFocus();
  });
});
