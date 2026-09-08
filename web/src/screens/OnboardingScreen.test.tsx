/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

vi.mock("../lib/vaultApi", () => ({
  updateAccountProfile: (...args: unknown[]) => apiPost(...(args as [])),
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
    expect(screen.queryByText(/Welcome to the Message Vault/i)).not.toBeInTheDocument();
  });

  it("shows an example in the empty value field", () => {
    render(<OnboardingScreen />);
    expect(rowValue(1)).toHaveAttribute("placeholder", "+1 555-123-4567");
  });

  it("changes the placeholder when the service picker changes", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.click(screen.getByRole("button", { name: "Text message Account 1 type" }));
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
    expect(screen.getByText("Enter a phone number like +1 555-123-4567.")).toBeInTheDocument();
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
    expect(screen.getByText("Enter a phone number like +1 555-123-4567.")).toBeInTheDocument();
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

  it("holds back Continue to vault until the value is an account", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.type(screen.getByRole("textbox", { name: "Display Name" }), "Matt");
    await user.type(rowValue(1), "notaphone");
    await user.click(screen.getByRole("button", { name: "Continue to vault" }));

    expect(apiPost).not.toHaveBeenCalled();
    expect(rowValue(1)).toHaveAttribute("aria-invalid", "true");
  });

  it("keeps Continue to vault disabled until there is a name and an account", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    const submit = screen.getByRole("button", { name: "Continue to vault" });
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
    await user.click(screen.getByRole("button", { name: "Text message Account 2 type" }));
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
    expect(screen.getByText("Enter a phone number like +1 555-123-4567.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Text message Account 1 type" }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await user.click(screen.getByRole("button", { name: "+ Add account" }));

    expect(screen.getByText("Enter an email address like you@example.com.")).toBeInTheDocument();
  });

  it("goes back one screen, to login", async () => {
    const user = setupUser();
    render(<OnboardingScreen />);

    await user.click(screen.getByRole("button", { name: "Back to login" }));
    expect(logout).toHaveBeenCalledOnce();
  });
});
