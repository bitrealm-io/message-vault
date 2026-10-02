/** @vitest-environment jsdom */

import { cleanup, screen, waitFor, within } from "@testing-library/react";
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
  getAccountProfile.mockResolvedValue(stored);
  // The server answers with the profile as it now stands: a new object.
  updateAccountProfile.mockImplementation(async (body: Partial<AccountProfile>) => ({
    ...stored,
    ...body,
  }));
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
});
