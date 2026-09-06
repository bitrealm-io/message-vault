/** @vitest-environment jsdom */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const login = vi.fn();
const setServer = vi.fn();

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ login, setServer, serverUrl: "" }),
}));

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => false,
}));

import { VaultProviders } from "../test/vaultProviders";
import LoginScreen from "./LoginScreen";

/**
 * Answer `/health` as a healthy vault and `/v1/vault` with the state given.
 *
 * The state decides which forms the card offers, so a test that says nothing
 * about it gets `open` — the two-tab card, which is what most of these tests
 * are about. Returns the underlying fetch mock.
 */
function stubVault(state: "unclaimed" | "closed" | "open" = "open") {
  // `/health` is read with `text()`; the API client reads `status` and
  // `json()`. Both shapes come back from the one stub so a test does not have
  // to know which of the two a given screen used.
  const fetchMock = vi.fn(async (url: string, _init?: RequestInit) => ({
    ok: true,
    status: 200,
    text: async () => (String(url).includes("/v1/vault") ? JSON.stringify({ state }) : ""),
    json: async () => ({ state }),
  }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function renderScreen() {
  render(
    <VaultProviders>
      <MemoryRouter>
        <LoginScreen />
      </MemoryRouter>
    </VaultProviders>,
  );
}

// user-event's default per-keystroke delay is a real setTimeout(0). Under a
// loaded machine that delay is scheduled, not skipped, so an address typed a
// character at a time can take much longer than the card's own 400ms health
// re-probe debounce — long enough for the background probe to race the
// explicit reconnect this screen triggers. `delay: null` fires every
// keystroke synchronously, closing that window regardless of machine load.
const setupUser = () => userEvent.setup({ delay: null });

describe("LoginScreen", () => {
  beforeEach(() => {
    login.mockReset();
    setServer.mockReset();
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("logs in without a vault-selection step", async () => {
    stubVault();
    renderScreen();

    expect(await screen.findByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Username" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();

    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Server URL" })).not.toBeInTheDocument();
  });

  it("names the product and reports the connection as one word", async () => {
    stubVault();
    renderScreen();

    expect(await screen.findByText("Connected")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Message Vault" })).toBeInTheDocument();
    expect(setServer).toHaveBeenCalledWith("");
  });

  it("never shows the vault's host address", async () => {
    stubVault();
    renderScreen();

    await screen.findByText("Connected");
    expect(screen.queryByText(/127\.0\.0\.1/)).not.toBeInTheDocument();
    expect(screen.queryByText(/localhost/)).not.toBeInTheDocument();
  });

  it("probes /health rather than the auth mode endpoint", async () => {
    const fetchMock = stubVault();
    renderScreen();

    await screen.findByText("Connected");

    const calls = fetchMock.mock.calls.map(([url]) => String(url));
    expect(calls.some((url) => url.endsWith("/health"))).toBe(true);
    expect(calls.some((url) => url.endsWith("/v1/auth/mode"))).toBe(false);
  });

  it("keeps both tabs, Login first", async () => {
    stubVault();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Login", "Create Account"]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
  });

  it("still asks for the password twice on Create Account", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Create Account" });
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("drops the password-length claim the server does not enforce", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Create Account" });
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.queryByText(/At least 8 characters/i)).not.toBeInTheDocument();
  });

  it("rejects a new account when the two passwords disagree", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Create Account" });
    await user.click(screen.getByRole("tab", { name: "Create Account" }));
    await user.type(screen.getByRole("textbox", { name: "Username" }), "ada");
    await user.type(screen.getByLabelText("Password"), "hunter22");
    await user.type(screen.getByLabelText("Confirm Password"), "hunter23");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    expect(await screen.findByText("Passwords do not match.")).toBeInTheDocument();
    expect(login).not.toHaveBeenCalled();
  });

  it("says Disconnected when nothing answers", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    renderScreen();

    expect(await screen.findByText("Disconnected")).toBeInTheDocument();
    // The address field belongs to the settings screen now, not the card.
    expect(screen.queryByRole("textbox", { name: "Address" })).not.toBeInTheDocument();
  });

  it("lets the vault be changed while the card is still connecting", async () => {
    // A vault that never answers holds the card in "connecting": a wrong
    // address is exactly when you need the settings screen most, so the way
    // to it must not wait for the probe to give up.
    vi.stubGlobal(
      "fetch",
      vi.fn(() => new Promise(() => {})),
    );
    const user = setupUser();
    renderScreen();

    expect(await screen.findByText("Connecting")).toBeInTheDocument();
    const link = screen.getByRole("button", { name: "Change vault settings" });
    expect(link).toBeEnabled();

    await user.click(link);
    expect(screen.getByRole("heading", { name: "Message Vault Settings" })).toBeInTheDocument();
  });

  it("keeps the way out of a red card live", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    renderScreen();

    await screen.findByText("Disconnected");
    expect(screen.getByRole("button", { name: "Change vault settings" })).toBeEnabled();
  });

  it("disables Log in while the vault is unreachable", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await user.type(field, "http://127.0.0.1:9999");
    await user.click(screen.getByRole("button", { name: "Change vault address" }));

    expect(await screen.findByText("Disconnected")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Login" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeDisabled();
  });

  it("offers Change vault address only for an address that is a change", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));

    const field = screen.getByRole("textbox", { name: "Address" });
    const apply = () => screen.getByRole("button", { name: "Change vault address" });

    // The card connected to the address the field already holds, so there is
    // nothing to apply.
    expect(apply()).toBeDisabled();

    await user.type(field, "http://127.0.0.1:8080");
    expect(apply()).toBeEnabled();

    // A field naming no address at all is nothing to apply either.
    await user.clear(field);
    expect(apply()).toBeDisabled();

    // Cancel is unaffected and stays the way out.
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();

    await user.type(field, "http://127.0.0.1:8080");
    await user.click(apply());
    await waitFor(() => {
      expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");
    });

    // Back on the settings screen, the applied address is now the connected
    // one, so it is no longer a change — and editing it makes it one again.
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("http://127.0.0.1:8080");
    expect(apply()).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: "Address" }), "9");
    expect(apply()).toBeEnabled();
  });

  it("opens Message Vault Settings from the link and comes back on Cancel", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));

    expect(screen.getByRole("heading", { name: "Message Vault Settings" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Address" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
    // The settings screen replaces the card body rather than opening beside it.
    expect(screen.queryByRole("tab", { name: "Login" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("tab", { name: "Login" })).toBeInTheDocument();
  });

  it("reports what Test found for the typed address", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await user.type(field, "http://127.0.0.1:9999");
    await user.click(screen.getByRole("button", { name: "Test" }));

    expect(await screen.findByText("Disconnected")).toBeInTheDocument();
    // Testing does not commit the address: the card is still connected behind.
    expect(setServer).not.toHaveBeenCalledWith("http://127.0.0.1:9999");
  });

  it("does not credit an edited address with the connection it never earned", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));
    // Opened on the address the card is connected to, so that connection is
    // this address's and saying so is true.
    expect(screen.getByRole("status")).toHaveTextContent("Connected");

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.type(field, "http://127.0.0.1:9999");
    // Typed but never tried: the card is still connected behind this screen,
    // but not to what is in the box.
    expect(screen.getByRole("status")).toHaveTextContent("Not tested");

    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("Disconnected")).toBeInTheDocument();

    // Editing after a failed test clears that answer without inventing a
    // better one. A green here would say the typed address works.
    await user.type(field, "9");
    expect(screen.getByRole("status")).toHaveTextContent("Not tested");
  });

  it("applies a typed address and reconnects", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    const user = setupUser();
    renderScreen();

    await screen.findByText("Disconnected");
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));

    // Only the address being typed answers healthy — the disconnected card's
    // own background self-heal probe (`useVaultHealth`) keeps polling the
    // blank address it was last on, a different host from the one typed
    // below. Two hosts, two answers, so this test is about the one thing it
    // names: applying the address that was typed. Which probe wins when both
    // answer is the next test's subject.
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async (
          url: string,
        ): Promise<{
          ok: boolean;
          status?: number;
          text: () => Promise<string>;
          json?: () => Promise<unknown>;
        }> => {
          if (String(url).startsWith("http://127.0.0.1:8080")) {
            // Healthy, and open, so the card offers the two tabs this test looks for.
            return {
              ok: true,
              status: 200,
              text: async () => JSON.stringify({ state: "open" }),
              json: async () => ({ state: "open" }),
            };
          }
          return { ok: false, status: 503, text: async () => "" };
        },
      ),
    );
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await user.type(field, "http://127.0.0.1:8080");
    await user.click(screen.getByRole("button", { name: "Change vault address" }));

    expect(await screen.findByRole("tab", { name: "Login" })).toBeInTheDocument();
    await waitFor(() => {
      expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");
    });
  });

  it("keeps the typed address when a slow probe for the old one answers late", async () => {
    // The card probes its saved address on mount. That probe is held open
    // here, so it is still in flight while a different address is typed and
    // submitted below — the two `connect()` calls the screen can have running
    // at once. The saved address is blank (`useAuth` above), which probes the
    // relative "/health"; the typed one is absolute, so the mock can tell them
    // apart and answer them in the order this test needs.
    let answerSavedAddress: (() => void) | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string): Promise<{ ok: boolean; text: () => Promise<string> }> => {
        if (String(url).startsWith("http://127.0.0.1:8080")) {
          return { ok: true, text: async () => "" };
        }
        return new Promise((resolve) => {
          answerSavedAddress = () => resolve({ ok: true, text: async () => "" });
        });
      }),
    );
    const user = setupUser();
    renderScreen();

    expect(await screen.findByText("Connecting")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));
    const field = screen.getByRole("textbox", { name: "Address" });
    await user.clear(field);
    await user.type(field, "http://127.0.0.1:8080");
    await user.click(screen.getByRole("button", { name: "Change vault address" }));

    await waitFor(() => {
      expect(setServer).toHaveBeenCalledWith("http://127.0.0.1:8080");
    });

    // Now the saved address answers healthy, after the typed one has already
    // been applied. It describes a vault this screen has moved on from, and a
    // probe nobody is waiting on any more may not speak for the card. A real
    // timer gives every pending microtask its chance to write first.
    answerSavedAddress?.();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(setServer).not.toHaveBeenCalledWith("");
    expect(setServer).toHaveBeenLastCalledWith("http://127.0.0.1:8080");

    // Reopening reads the address back out of the card, which is what the
    // late probe would have rewritten.
    await user.click(screen.getByRole("button", { name: "Change vault settings" }));
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("http://127.0.0.1:8080");
  });

  it("reconnects on its own once a probe finds the vault healthy again", async () => {
    let healthy = false;
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        return healthy
          ? {
              ok: true,
              status: 200,
              text: async () => JSON.stringify({ state: "open" }),
              json: async () => ({ state: "open" }),
            }
          : { ok: false, status: 503 };
      }),
    );
    renderScreen();

    await screen.findByText("Disconnected");

    healthy = true;

    expect(
      await screen.findByRole("tab", { name: "Login" }, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Connected")).toBeInTheDocument();
  });

  it("carries an abort signal on the health probe", async () => {
    const fetchMock = stubVault();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    const healthCall = fetchMock.mock.calls.find(([url]) => String(url).endsWith("/health"));
    expect(healthCall).toBeDefined();
    expect(healthCall?.[1]).toMatchObject({ signal: expect.any(AbortSignal) });
  });

  it("puts the credentials in a real form, so a password manager can fill it", async () => {
    stubVault();
    renderScreen();

    const password = await screen.findByLabelText("Password");
    // A password field outside a form is one browsers decline to offer to save.
    expect(password.closest("form")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Log in" })).toHaveAttribute("type", "submit");
  });

  // The fields carry no Enter handler of their own any more: submitting is the
  // form's job, which is what lets the browser submit on Enter by itself.
  // jsdom does not perform that implicit submission, so this drives the form
  // element directly — that the key reaches it is the browser's part.
  it("runs the sign-in from the form's own submit event", async () => {
    stubVault();
    renderScreen();

    await screen.findByRole("tab", { name: "Login" });
    const form = screen.getByLabelText("Password").closest("form");
    expect(form).not.toBeNull();

    fireEvent.submit(form as HTMLFormElement);

    // Submitting with the username still empty reaches the handler's own check,
    // which is enough to show the form is what drives it.
    expect(await screen.findByText("Username is required.")).toBeInTheDocument();
  });

  it("calls the new-account action Continue, since profile setup finishes it", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Create Account" });
    await user.click(screen.getByRole("tab", { name: "Create Account" }));

    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create account" })).not.toBeInTheDocument();
  });

  it("keeps the action under the fields and the error down by the or-rule", async () => {
    stubVault();
    const user = setupUser();
    renderScreen();

    await screen.findByRole("tab", { name: "Create Account" });
    await user.click(screen.getByRole("tab", { name: "Create Account" }));
    await user.type(screen.getByRole("textbox", { name: "Username" }), "ada");
    await user.type(screen.getByLabelText("Password"), "hunter22");
    await user.type(screen.getByLabelText("Confirm Password"), "hunter23");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    const message = await screen.findByText("Passwords do not match.");
    const confirmField = screen.getByLabelText("Confirm Password");
    const action = screen.getByRole("button", { name: "Continue" });
    const orRule = screen.getByText("or");

    // Document order stands in for the layout: field, then action, then the
    // message, then the rule that closes the card.
    const precedes = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(precedes(confirmField, action)).toBe(true);
    expect(precedes(action, message)).toBe(true);
    expect(precedes(message, orRule)).toBe(true);
  });
});
