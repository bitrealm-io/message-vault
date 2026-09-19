/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountProfile } from "../../lib/account";
import { AccountPermissionsSection } from "./AccountPermissionsSection";

vi.mock("../owner/useOwnerAccounts", () => ({
  useUpdateAccount: () => ({ mutate: vi.fn(), isPending: false, error: null }),
}));

const profile = {
  account_id: 101,
  username: "bob",
  disabled: false,
  can_import: true,
  can_export: true,
  can_delete: false,
} as AccountProfile;

afterEach(cleanup);

describe("AccountPermissionsSection", () => {
  it("shows the account holder their status and permissions with nothing to change", () => {
    render(<AccountPermissionsSection profile={profile} />);

    expect(screen.getByRole("heading", { name: "Status" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Permissions" })).toBeInTheDocument();
    // Status reads as text: a dropdown that never opens is not a thing to show.
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Status/ })).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Import messages" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Delete messages and attachments" }),
    ).not.toBeChecked();
    for (const box of screen.getAllByRole("checkbox")) expect(box).toBeDisabled();
    expect(
      screen.getByText("The vault owner sets your status and permissions."),
    ).toBeInTheDocument();
  });

  it("lets the vault owner change them on an account it opened", () => {
    render(<AccountPermissionsSection profile={profile} managedAccountId={101} />);

    expect(screen.getByRole("button", { name: /Status/ })).toBeEnabled();
    for (const box of screen.getAllByRole("checkbox")) expect(box).toBeEnabled();
    expect(
      screen.queryByText("The vault owner sets your status and permissions."),
    ).not.toBeInTheDocument();
  });
});
