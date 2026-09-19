/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthGuard } from "./AuthGuard";

const profileState = vi.hoisted(() => ({
  profile: null as {
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
        <Route path="/onboarding" element={<div>onboarding</div>} />
        <Route path="/owner" element={<div>owner home</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("AuthGuard", () => {
  it("sends an account that owes a profile to onboarding", () => {
    profileState.profile = { must_set_up_profile: true };
    renderGuard();

    expect(screen.getByText("onboarding")).toBeInTheDocument();
  });

  it("sends the vault owner to Owner Home, not into the message shell", () => {
    profileState.profile = { is_owner: true };
    renderGuard();

    expect(screen.getByText("owner home")).toBeInTheDocument();
    expect(screen.queryByText("the vault")).not.toBeInTheDocument();
  });

  it("lets an account that owes nothing through", () => {
    profileState.profile = {};
    renderGuard();

    expect(screen.getByText("the vault")).toBeInTheDocument();
  });

  it("renders nothing while the profile is still loading", () => {
    profileState.loading = true;
    renderGuard();

    // Not the app: showing it and redirecting after would flash a screen this
    // account has not finished earning.
    expect(screen.queryByText("the vault")).not.toBeInTheDocument();
    expect(screen.queryByText("onboarding")).not.toBeInTheDocument();
  });

  it("sends a logged-out visitor to the login screen before reading a profile", () => {
    authState.isAuthenticated = false;
    profileState.loading = true;
    renderGuard();

    expect(screen.getByText("login")).toBeInTheDocument();
  });
});
