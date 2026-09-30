/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Providers } from "../../test/providers";
import LocalAuthTabs from "./LocalAuthTabs";

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ login: vi.fn(), setServer: vi.fn(), serverUrl: "" }),
}));

afterEach(cleanup);

function renderTabs(serverState: "unclaimed" | "closed" | "open") {
  render(
    <Providers>
      <MemoryRouter>
        <LocalAuthTabs serverUrl="http://127.0.0.1:8080" serverState={serverState} />
      </MemoryRouter>
    </Providers>,
  );
}

/**
 * The card offers what the vault says is on offer. These three cases are the
 * whole of the entry screen's behaviour, and the reason the vault reports one
 * value rather than the facts behind it: the mapping lives in one place.
 */
describe("LocalAuthTabs", () => {
  it("offers only Create Owner on an unclaimed vault", () => {
    renderTabs("unclaimed");

    expect(screen.getByRole("button", { name: "Create Owner" })).toBeInTheDocument();
    // No login: there is no account to log into yet.
    expect(screen.queryByRole("button", { name: "Log in" })).not.toBeInTheDocument();
    // And no way to join a vault that has nobody to decide who may join it.
    expect(screen.queryByRole("tab", { name: "Create Account" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Login" })).not.toBeInTheDocument();
  });

  it("offers only Login on a closed vault", () => {
    renderTabs("closed");

    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Create Account" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create Owner" })).not.toBeInTheDocument();
  });

  it("offers Login and Create Account on an open vault, Login first", () => {
    renderTabs("open");

    const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
    expect(tabs).toEqual(["Login", "Create Account"]);
    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create Owner" })).not.toBeInTheDocument();
  });

  it("asks the vault owner for a username and the password twice", () => {
    renderTabs("unclaimed");

    expect(screen.getByRole("textbox", { name: "Username" })).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm Password")).toBeInTheDocument();
  });

  it("defaults the owner's username to admin, and lets it be changed", () => {
    renderTabs("unclaimed");

    const field = screen.getByRole("textbox", { name: "Username" }) as HTMLInputElement;
    expect(field.value).toBe("admin");
    expect(field.readOnly).toBe(false);
  });
});
