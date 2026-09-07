/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthGuard } from "./AuthGuard";

const profileState = vi.hoisted(() => ({
  profile: null as {
    must_change_password: boolean;
    must_set_up_profile?: boolean;
    is_owner?: boolean;
  } | null,
  loading: false,
}));
const authState = vi.hoisted(() => ({ isAuthenticated: true }));

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({
    profile: profileState.profile,
    loading: profileState.loading,
    error: "",
  }),
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => authState,
}));

afterEach(() => {
  cleanup();
  profileState.profile = null;
  profileState.loading = false;
  authState.isAuthenticated = true;
});

function renderGuard() {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route element={<AuthGuard />}>
          <Route path="/" element={<div>the vault</div>} />
        </Route>
        <Route path="/login" element={<div>login</div>} />
        <Route path="/set-password" element={<div>set password</div>} />
        <Route path="/onboarding" element={<div>onboarding</div>} />
        <Route path="/admin" element={<div>owner console</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

/**
 * The guard's order is an order of urgency: an account still carrying the
 * password the vault owner chose is using a credential someone else knows,
 * which matters more than not having named itself yet.
 */
describe("AuthGuard", () => {
  it("sends an account that owes a password change to set one", () => {
    profileState.profile = { must_change_password: true };
    renderGuard();

    expect(screen.getByText("set password")).toBeInTheDocument();
  });

  it("puts the password before profile setup when both are owed", () => {
    profileState.profile = { must_change_password: true, must_set_up_profile: true };
    renderGuard();

    expect(screen.getByText("set password")).toBeInTheDocument();
    expect(screen.queryByText("onboarding")).not.toBeInTheDocument();
  });

  it("sends an account that only owes a profile to onboarding", () => {
    profileState.profile = { must_change_password: false, must_set_up_profile: true };
    renderGuard();

    expect(screen.getByText("onboarding")).toBeInTheDocument();
  });

  it("sends the vault owner to their console, not into the message shell", () => {
    profileState.profile = { must_change_password: false, is_owner: true };
    renderGuard();

    expect(screen.getByText("owner console")).toBeInTheDocument();
    expect(screen.queryByText("the vault")).not.toBeInTheDocument();
  });

  it("makes the owner set a password it still owes before reaching the console", () => {
    profileState.profile = { must_change_password: true, is_owner: true };
    renderGuard();

    expect(screen.getByText("set password")).toBeInTheDocument();
  });

  it("lets an account that owes nothing through", () => {
    profileState.profile = { must_change_password: false };
    renderGuard();

    expect(screen.getByText("the vault")).toBeInTheDocument();
  });

  it("renders nothing while the profile is still loading", () => {
    profileState.loading = true;
    renderGuard();

    // Not the app: showing it and redirecting after would flash a screen this
    // account has not finished earning.
    expect(screen.queryByText("the vault")).not.toBeInTheDocument();
    expect(screen.queryByText("set password")).not.toBeInTheDocument();
  });

  it("sends a signed-out visitor to the login screen before reading a profile", () => {
    authState.isAuthenticated = false;
    profileState.loading = true;
    renderGuard();

    expect(screen.getByText("login")).toBeInTheDocument();
  });
});
