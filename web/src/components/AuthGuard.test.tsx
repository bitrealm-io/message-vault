/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthGuard } from "./AuthGuard";

const profileState = vi.hoisted(() => ({
  profile: null as {
    must_set_up_profile?: boolean;
    is_owner?: boolean;
  } | null,
  loading: false,
  error: "",
}));
const retry = vi.hoisted(() => vi.fn());
const authState = vi.hoisted(() => ({ isAuthenticated: true }));

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({
    profile: profileState.profile,
    loading: profileState.loading,
    error: profileState.error,
    retry,
  }),
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => authState,
}));

afterEach(() => {
  cleanup();
  profileState.profile = null;
  profileState.loading = false;
  profileState.error = "";
  retry.mockReset();
  authState.isAuthenticated = true;
});

function renderGuard() {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route element={<AuthGuard />}>
          <Route path="/" element={<div>the messages</div>} />
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

  it("sends the owner to Owner Home, not into the message shell", () => {
    profileState.profile = { is_owner: true };
    renderGuard();

    expect(screen.getByText("owner home")).toBeInTheDocument();
    expect(screen.queryByText("the messages")).not.toBeInTheDocument();
  });

  it("does not let an account in whose profile could not be loaded", () => {
    // What useAccountProfile reports once GET /v1/account/profile has failed.
    profileState.profile = null;
    profileState.loading = false;
    profileState.error = "Internal Server Error";
    renderGuard();

    expect(screen.queryByText("the messages")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Internal Server Error");
  });

  it("asks for the profile again from the error screen", async () => {
    profileState.error = "Internal Server Error";
    renderGuard();

    await userEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("lets an account that owes nothing through", () => {
    profileState.profile = {};
    renderGuard();

    expect(screen.getByText("the messages")).toBeInTheDocument();
  });

  it("renders nothing while the profile is still loading", () => {
    profileState.loading = true;
    renderGuard();

    // Not the app: showing it and redirecting after would flash a screen this
    // account has not finished earning.
    expect(screen.queryByText("the messages")).not.toBeInTheDocument();
    expect(screen.queryByText("onboarding")).not.toBeInTheDocument();
  });

  it("sends a logged-out visitor to the login screen before reading a profile", () => {
    authState.isAuthenticated = false;
    profileState.loading = true;
    renderGuard();

    expect(screen.getByText("login")).toBeInTheDocument();
  });
});
