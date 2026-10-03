/** @vitest-environment jsdom */

import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProfile } from "../../lib/account";
import { mockedAuth, renderWithProviders as render } from "../../test/providers";
import { ProfileSettingsPanel } from "./ProfileSettingsPanel";

const getAccountProfile = vi.hoisted(() => vi.fn());
const updateAccountProfile = vi.hoisted(() => vi.fn());
vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  updateAccountProfile: (...a: unknown[]) => updateAccountProfile(...a),
}));

// The owner's profile, so the identities and address book sections stay out
// of the way: the bug is between the name field and the time zone.
const stored = {
  account_id: 7,
  username: "owner",
  preferred_name: "Stored Name",
  time_zone: "Etc/UTC",
  is_owner: true,
  phones: [],
  emails: [],
} as unknown as AccountProfile;

beforeEach(() => {
  getAccountProfile.mockReset();
  updateAccountProfile.mockReset();
  // The server keeps what a write stored, so the read every write starts
  // answers the new profile, as the server does.
  let current = stored;
  getAccountProfile.mockImplementation(async () => ({ ...current }));
  // The server answers with the profile as it now stands: a new object. A
  // field the body leaves out, as JSON leaves out `undefined`, stays as it
  // was; `null` clears it.
  updateAccountProfile.mockImplementation(async (body: Partial<AccountProfile>) => {
    const sent = Object.fromEntries(Object.entries(body).filter(([, v]) => v !== undefined));
    current = { ...current, ...sent };
    return { ...current };
  });
});
afterEach(cleanup);

function nameField() {
  return screen.getByRole("textbox", { name: "Display name" }) as HTMLInputElement;
}

function zoneField() {
  return screen.getByRole("combobox", { name: "Time zone" }) as HTMLInputElement;
}

describe("ProfileSettingsPanel", () => {
  it("keeps a typed, unsaved display name when the time zone changes", async () => {
    render(<ProfileSettingsPanel />);
    await waitFor(() => expect(nameField().value).toBe("Stored Name"));

    await userEvent.clear(nameField());
    await userEvent.type(nameField(), "Typed Name");

    await userEvent.click(zoneField());
    await userEvent.keyboard("dallas");
    await userEvent.click(within(screen.getByRole("listbox")).getAllByRole("option")[0]);
    expect(updateAccountProfile).toHaveBeenCalledWith({ time_zone: "America/Chicago" });
    // The server's answer has reached the profile entry the panel reads.
    await waitFor(() => expect(zoneField().value).toMatch(/Central Time/));

    expect(nameField().value).toBe("Typed Name");
  });

  it("clears the display name when an emptied field is saved", async () => {
    render(<ProfileSettingsPanel />);
    await waitFor(() => expect(nameField().value).toBe("Stored Name"));

    await userEvent.clear(nameField());
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    // The server clears the name on `null`; leaving the field out keeps it.
    expect(updateAccountProfile).toHaveBeenCalledWith({ preferred_name: null });
    // Once the answer arrives the field shows the name it carries, so the
    // field reads empty only when the answer has no name.
    await act(async () => {
      await updateAccountProfile.mock.results[0].value;
    });
    expect(nameField().value).toBe("");
  });

  it("shows the Demo Account's name and zone as fixed and offers no address book load", async () => {
    getAccountProfile.mockImplementation(async () => ({
      ...stored,
      username: "demo",
      preferred_name: "Demo User",
      is_owner: false,
      is_demo: true,
    }));
    render(<ProfileSettingsPanel />);
    await waitFor(() => expect(nameField().value).toBe("Demo User"));

    // The server refuses each of these for the Demo Account, so none is offered.
    expect(nameField().readOnly).toBe(true);
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(zoneField().disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Choose a file" })).toBeNull();
    expect(screen.getByText(/display name and time zone are fixed/)).toBeTruthy();
  });
});
