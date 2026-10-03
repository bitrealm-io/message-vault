/** @vitest-environment jsdom */

import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders as render } from "../test/providers";

const logout = vi.fn();
const apiPost = vi.fn(async () => ({}));

vi.mock("../lib/auth", () => ({
  useAuth: () => ({
    login: vi.fn(),
    logout,
    token: "t",
    serverUrl: "",
    accountId: 7,
  }),
}));

// The account's identities as the server lists them, each with its service; a
// test sets them to what the owner added.
type Service = "phone" | "email" | "whatsapp";
let identities: { address: string; service: Service }[] = [];

vi.mock("../lib/serverApi", () => ({
  updateAccountProfile: (...args: unknown[]) => apiPost(...(args as [])),
  listAccountIdentities: async () =>
    identities.map(({ address, service }) => ({
      address,
      service,
      start_date: null,
      end_date: null,
      conversations: 0,
      direct_messages: 0,
      group_messages: 0,
    })),
}));

// What the server says the account already holds; a test sets it to what the owner filled in.
const blankProfile = { preferred_name: null, time_zone: "UTC" };
let profile: { preferred_name: string | null; time_zone: string } = blankProfile;

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile, loading: false, error: "" }),
}));

import OnboardingScreen, { SAME_GESTURE_MS } from "./OnboardingScreen";

const rowValue = (n: number) => screen.getByRole("textbox", { name: `Account ${n} value` });

// user-event's default per-keystroke delay is a real setTimeout(0). Under a
// loaded machine that delay is scheduled, not skipped, so it can stretch a
// row full of typing well past a moment even though nothing here is actually
// waiting on anything. `delay: null` fires every keystroke synchronously, so
// how busy the machine is stops being able to slow these tests down.
const setupUser = () => userEvent.setup({ delay: null });

describe("OnboardingScreen", () => {
  beforeEach(() => {
    logout.mockReset();
    apiPost.mockReset();
    profile = blankProfile;
    identities = [];
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("names the section Your Accounts and explains nothing further", () => {
    render(<OnboardingScreen />);

    expect(screen.getByText("Your Accounts")).toBeInTheDocument();
    expect(screen.queryByText(/How you show up/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Source Accounts/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Welcome to the Message Crate/i)).not.toBeInTheDocument();
  });

  it("renders and adds rows where the browser has no crypto.randomUUID", async () => {
    // Browsers expose crypto.randomUUID only on HTTPS and localhost, so a
    // Message Crate opened at http://192.168.x.x has none.
    Object.defineProperty(crypto, "randomUUID", { value: undefined, configurable: true });
    try {
      const user = setupUser();
      render(<OnboardingScreen />);

      await user.type(rowValue(1), "+1 555-123-4567");
      await user.click(screen.getByRole("button", { name: "+ Add account" }));
      await user.type(rowValue(2), "+1 555-123-4568");

      expect(rowValue(1)).toHaveValue("+1 555-123-4567");
      expect(rowValue(2)).toHaveValue("+1 555-123-4568");
    } finally {
      // Drop the shadowing property so the real method shows through again.
      delete (crypto as { randomUUID?: unknown }).randomUUID;
    }
  });

  it("shows the name and zone the owner set, for the holder to check", () => {
    profile = { ...blankProfile, preferred_name: "Bob Archer", time_zone: "Asia/Tokyo" };
    render(<OnboardingScreen />);

    expect(screen.getByRole("textbox", { name: "Display Name" })).toHaveValue("Bob Archer");
    expect(screen.getByRole("combobox", { name: "Time Zone" })).toHaveValue(
      "(UTC+09:00) Japan Time \u2014 Tokyo, Yokohama",
    );
  });

  it("starts on this browser's zone when nobody has chosen one", () => {
    render(<OnboardingScreen />);
    expect(screen.getByRole("textbox", { name: "Display Name" })).toHaveValue("");
    expect(screen.getByRole("combobox", { name: "Time Zone" })).not.toHaveValue("");
  });

  it("puts each identity the owner added in its own row, for the holder to change", async () => {
    identities = [
      { address: "+15555550100", service: "phone" },
      { address: "bob@example.com", service: "email" },
    ];
    render(<OnboardingScreen />);

    expect(await screen.findByRole("button", { name: "Email Account 2 type" })).toBeInTheDocument();
    expect(rowValue(1)).toHaveValue("+15555550100");
    expect(rowValue(2)).toHaveValue("bob@example.com");
    expect(screen.queryByText(/Already on this account/)).not.toBeInTheDocument();
  });

  it("shows a number on Text Message and on WhatsApp as two rows, and does not block Continue", async () => {
    profile = { ...blankProfile, preferred_name: "Bob" };
    identities = [
      { address: "+15555550123", service: "phone" },
      { address: "+15555550123", service: "whatsapp" },
    ];
    const user = setupUser();
    render(<OnboardingScreen />);

    expect(
      await screen.findByRole("button", { name: "WhatsApp Account 2 type" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Text Message Account 1 type" })).toBeInTheDocument();
    expect(rowValue(1)).toHaveValue("+15555550123");
    expect(rowValue(2)).toHaveValue("+15555550123");

    const go = screen.getByRole("button", { name: "Continue to Message Crate" });
    expect(go).toBeEnabled();
    await user.click(go);
    expect(screen.queryByText("This account is already in the list.")).not.toBeInTheDocument();
    await waitFor(() => expect(apiPost).toHaveBeenCalled());
    expect(apiPost).toHaveBeenCalledWith(
      expect.objectContaining({ identities: [], remove_identities: [] }),
    );
  });

  it("shows an identity that is on WhatsApp only as WhatsApp", async () => {
    identities = [{ address: "+15555550123", service: "whatsapp" }];
    render(<OnboardingScreen />);

    expect(
      await screen.findByRole("button", { name: "WhatsApp Account 1 type" }),
    ).toBeInTheDocument();
    expect(rowValue(1)).toHaveValue("+15555550123");
  });

  it("counts an identity the owner added, so another is not required", async () => {
    identities = [{ address: "+15555550100", service: "phone" }];
    render(<OnboardingScreen />);
    await waitFor(() => expect(rowValue(1)).toHaveValue("+15555550100"));

    const go = screen.getByRole("button", { name: "Continue to Message Crate" });
    expect(go).toBeDisabled();
    await setupUser().type(screen.getByRole("textbox", { name: "Display Name" }), "Bob");
    expect(go).toBeEnabled();
  });

  it("sends only what changed: an edited identity is unlinked and its new value linked", async () => {
    profile = { ...blankProfile, preferred_name: "Bob" };
    identities = [
      { address: "+15555550100", service: "phone" },
      { address: "+15555550101", service: "phone" },
    ];
    const user = setupUser();
    render(<OnboardingScreen />);
    await waitFor(() => expect(rowValue(2)).toHaveValue("+15555550101"));

    await user.clear(rowValue(1));
    await user.paste("+1 555-555-0199");
    await user.click(screen.getByRole("button", { name: "Continue to Message Crate" }));

    await waitFor(() => expect(apiPost).toHaveBeenCalled());
    expect(apiPost).toHaveBeenCalledWith(
      expect.objectContaining({
        identities: [{ address: "+1 555-555-0199", service: "phone" }],
        remove_identities: [{ address: "+15555550100", service: "phone" }],
      }),
    );
  });

  it("unlinks an identity whose row is removed", async () => {
    profile = { ...blankProfile, preferred_name: "Bob" };
    identities = [
      { address: "+15555550100", service: "phone" },
      { address: "+15555550101", service: "phone" },
    ];
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.click(await screen.findByRole("button", { name: "Remove account 2" }));
    await user.click(screen.getByRole("button", { name: "Continue to Message Crate" }));

    await waitFor(() => expect(apiPost).toHaveBeenCalled());
    expect(apiPost).toHaveBeenCalledWith(
      expect.objectContaining({
        identities: [],
        remove_identities: [{ address: "+15555550101", service: "phone" }],
      }),
    );
  });

  it("shows no more identities than the card has rows for, and leaves the rest alone", async () => {
    profile = { ...blankProfile, preferred_name: "Bob" };
    identities = [
      { address: "+15555550100", service: "phone" },
      { address: "+15555550101", service: "phone" },
      { address: "+15555550102", service: "phone" },
      { address: "+15555550103", service: "phone" },
      { address: "bob@example.com", service: "email" },
      { address: "b@example.com", service: "email" },
    ];
    const user = setupUser();
    render(<OnboardingScreen />);

    expect(await screen.findByText("2 more are in Settings.")).toBeInTheDocument();
    expect(rowValue(4)).toHaveValue("+15555550103");
    expect(screen.queryByRole("textbox", { name: "Account 5 value" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Continue to Message Crate" }));
    await waitFor(() => expect(apiPost).toHaveBeenCalled());
    expect(apiPost).toHaveBeenCalledWith(
      expect.objectContaining({ identities: [], remove_identities: [] }),
    );
  });

  it("shows an example in the empty value field", () => {
    render(<OnboardingScreen />);
    expect(rowValue(1)).toHaveAttribute("placeholder", "+1 555-123-4567");
  });

  it("changes the placeholder when the service picker changes", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.click(screen.getByRole("button", { name: "Text Message Account 1 type" }));
    await user.click(screen.getByRole("option", { name: "Email" }));

    expect(rowValue(1)).toHaveAttribute("placeholder", "you@example.com");
  });

  it("hides the remove control until there is more than one row", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    expect(screen.queryByRole("button", { name: "Remove account 1" })).not.toBeInTheDocument();

    await user.type(rowValue(1), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    expect(screen.getByRole("button", { name: "Remove account 1" })).toBeInTheDocument();
    expect(rowValue(2)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove account 2" }));
    expect(screen.queryByRole("button", { name: "Remove account 1" })).not.toBeInTheDocument();
  });

  it("stops at four accounts and points at Settings", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    // Each row has to hold a distinct account before the next one can be
    // added. Typing three of them a character at a time is real synchronous
    // rendering work — one React update per keystroke — with no timer or
    // wait involved, so a busy machine can push it past a wall-clock budget
    // on its own. Pasting each value in one event drives the same
    // validation with a fraction of the renders, which is what actually
    // keeps this fast under load rather than just giving it more time to
    // finish in.
    for (let i = 0; i < 3; i++) {
      const field = rowValue(i + 1);
      await user.click(field);
      await user.paste(`+1 555-123-45${60 + i}`);
      await user.click(screen.getByRole("button", { name: "+ Add account" }));
    }

    expect(rowValue(4)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ Add account" })).not.toBeInTheDocument();
    expect(screen.getByText("Add the rest in Settings after setup.")).toBeInTheDocument();
  });

  it("will not add a row on top of a value that is not an account", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    // The row is refused, marked, and said out loud — all three, or the person
    // is left guessing why nothing happened.
    expect(screen.queryByRole("textbox", { name: "Account 2 value" })).not.toBeInTheDocument();
    expect(rowValue(1)).toHaveAttribute("aria-invalid", "true");
    // `findByText`, not `getByText`: when the same message is asked for a
    // second time outside one gesture, the screen clears the line and brings
    // it back after REPEATED_ERROR_BLINK_MS so the person can see the field
    // was rechecked. Typing is slow enough on a loaded machine to cross that
    // threshold, and a synchronous read lands in the blank window.
    expect(
      await screen.findByText("Enter a phone number like +1 555-123-4567."),
    ).toBeInTheDocument();
  });

  it("adds the row once the value is corrected", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    expect(screen.queryByRole("textbox", { name: "Account 2 value" })).not.toBeInTheDocument();

    await user.clear(rowValue(1));
    await user.type(rowValue(1), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    expect(screen.getByRole("textbox", { name: "Account 2 value" })).toBeInTheDocument();
    expect(rowValue(1)).not.toHaveAttribute("aria-invalid", "true");
  });

  it("checks a value when the field is left", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("textbox", { name: "Display Name" }));

    expect(rowValue(1)).toHaveAttribute("aria-invalid", "true");
    // `findByText`, not `getByText`: when the same message is asked for a
    // second time outside one gesture, the screen clears the line and brings
    // it back after REPEATED_ERROR_BLINK_MS so the person can see the field
    // was rechecked. Typing is slow enough on a loaded machine to cross that
    // threshold, and a synchronous read lands in the blank window.
    expect(
      await screen.findByText("Enter a phone number like +1 555-123-4567."),
    ).toBeInTheDocument();
  });

  it("keeps the mark on the row that earned it when another is removed", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    await user.type(rowValue(2), "notaphone");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    expect(rowValue(2)).toHaveAttribute("aria-invalid", "true");

    // Row 1 goes, so the bad value is row 1 now. The mark has to move with the
    // value, not stay on the position it was first found at.
    await user.click(screen.getByRole("button", { name: "Remove account 1" }));

    expect(rowValue(1)).toHaveValue("notaphone");
    expect(rowValue(1)).toHaveAttribute("aria-invalid", "true");
  });

  it("holds back Continue to Message Crate until the value is an account", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(screen.getByRole("textbox", { name: "Display Name" }), "Matt");
    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "Continue to Message Crate" }));

    expect(apiPost).not.toHaveBeenCalled();
    expect(rowValue(1)).toHaveAttribute("aria-invalid", "true");
  });

  it("keeps Continue to Message Crate disabled until there is a name and an account", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    const submit = screen.getByRole("button", { name: "Continue to Message Crate" });
    expect(submit).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: "Display Name" }), "Matt");
    expect(submit).toBeDisabled();

    await user.type(rowValue(1), "+1 555-123-4567");
    expect(submit).toBeEnabled();
  });

  it("will not offer to add a row while the one above it is empty", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    const add = screen.getByRole("button", { name: "+ Add account" });
    expect(add).toBeDisabled();

    await user.type(rowValue(1), "+1 555-123-4567");
    expect(add).toBeEnabled();

    await user.clear(rowValue(1));
    expect(add).toBeDisabled();
  });

  it("refuses an account already in the list and blames the later row", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    // The same number, typed the other way — still the same account.
    await user.type(rowValue(2), "+15551234567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    expect(screen.getByText("This account is already in the list.")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Account 3 value" })).not.toBeInTheDocument();
    // The first row to carry the number is not the mistake; the repeat is.
    expect(rowValue(2)).toHaveAttribute("aria-invalid", "true");
    expect(rowValue(1)).not.toHaveAttribute("aria-invalid", "true");
  });

  it("allows the same number on two different services", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    await user.click(screen.getByRole("button", { name: "Text Message Account 2 type" }));
    await user.click(screen.getByRole("option", { name: "WhatsApp" }));
    await user.type(rowValue(2), "+1 555-123-4567");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    expect(screen.queryByText("This account is already in the list.")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Account 3 value" })).toBeInTheDocument();
  });

  it("clears a repeated message before showing it again, so the recheck is visible", async () => {
    // The screen tells a second look from the blur-then-press pair of a
    // single click by comparing real Date.now() gaps (SAME_GESTURE_MS). This
    // pins the clock with `vi.setSystemTime` and jumps it forward instead of
    // waiting on the wall clock, so that comparison lands the same way no
    // matter how busy the machine is. `vi.useFakeTimers()` would let the gap
    // be advanced synchronously too, but user-event's `type()` hangs when
    // Vitest's fake timers are active (independent of this screen, and
    // reproducible on a bare `<input>`), so `setTimeout` stays real here —
    // the message's REPEATED_ERROR_BLINK_MS return is still awaited for real,
    // just with no artificial wait stacked in front of it.
    const user = setupUser();
    vi.setSystemTime(Date.now());
    render(<OnboardingScreen />);

    const message = "Enter a phone number like +1 555-123-4567.";
    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    expect(screen.getByText(message)).toBeInTheDocument();

    // Far enough after the first click to be a second look rather than the
    // blur-then-press pair that a single click produces.
    vi.setSystemTime(Date.now() + SAME_GESTURE_MS + 50);

    // The same words landing again would otherwise look like nothing happened.
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    expect(screen.queryByText(message)).not.toBeInTheDocument();

    await waitFor(() => expect(screen.getByText(message)).toBeInTheDocument());
  });

  it("swaps straight to a different message without blanking first", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "+ Add account" }));
    // `findByText`, not `getByText`: when the same message is asked for a
    // second time outside one gesture, the screen clears the line and brings
    // it back after REPEATED_ERROR_BLINK_MS so the person can see the field
    // was rechecked. Typing is slow enough on a loaded machine to cross that
    // threshold, and a synchronous read lands in the blank window.
    expect(
      await screen.findByText("Enter a phone number like +1 555-123-4567."),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Text Message Account 1 type" }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    expect(
      await screen.findByText("Enter an email address like you@example.com."),
    ).toBeInTheDocument();
  });

  it("goes back one screen, to login", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.click(screen.getByRole("button", { name: "Back to login" }));
    expect(logout).toHaveBeenCalledOnce();
  });
});
