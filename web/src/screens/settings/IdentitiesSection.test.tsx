/** @vitest-environment jsdom */

import { cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProfile } from "../../lib/account";
import { mockedAuth, renderWithVault as render } from "../../test/vaultProviders";
import { IdentitiesSection } from "./IdentitiesSection";
import { type Identity, removeBody } from "./identities";

const mutateAsync = vi.hoisted(() => vi.fn());
const listAccountIdentities = vi.hoisted(() => vi.fn());
vi.mock("../../lib/useSettingsAccount", () => ({
  useUpdateSettingsProfile: () => ({ mutateAsync, isPending: false }),
}));
vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));
vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  listAccountIdentities: (...a: unknown[]) => listAccountIdentities(...a),
}));

const profile = {
  account_id: 101,
  username: "bob",
  phones: ["+15555550100"],
  emails: ["bob@example.com", "archer@example.com"],
} as AccountProfile;

/** What the vault lists for `profile`, with the messages held at each identity. */
const identities: Identity[] = [
  {
    address: "+15555550100",
    service: "phone",
    start_date: "2020-01-01T00:00:00Z",
    end_date: "2020-02-03T00:00:00Z",
    conversations: 2,
    direct_messages: 12,
    group_messages: 30,
  },
  {
    address: "bob@example.com",
    service: "email",
    start_date: "2021-06-01T00:00:00Z",
    end_date: "2021-06-01T00:00:00Z",
    conversations: 1,
    direct_messages: 1,
    group_messages: 0,
  },
  {
    address: "archer@example.com",
    service: "email",
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
  },
];

beforeEach(() => {
  mutateAsync.mockReset();
  listAccountIdentities.mockReset();
  listAccountIdentities.mockResolvedValue({
    items: identities,
    total: identities.length,
    limit: 40,
    offset: 0,
  });
});
afterEach(cleanup);

describe("IdentitiesSection", () => {
  it("says what identities are for, then lists them with dates and counts from the vault", async () => {
    render(<IdentitiesSection profile={profile} />);

    expect(screen.getByRole("heading", { name: "My Identities" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Your phone numbers and emails. Import uses them to determine which messages belong to you.",
      ),
    ).toBeInTheDocument();

    const table = screen.getByRole("grid", { name: "Identities" });
    for (const name of [
      /Service/,
      /^Identity/,
      /First seen/,
      /Last seen/,
      /Conversations/,
      /Direct messages/,
      /Group messages/,
    ]) {
      expect(within(table).getByRole("columnheader", { name })).toBeInTheDocument();
    }
    expect(within(table).getByText("Text message")).toBeInTheDocument();
    expect(within(table).getByText("bob@example.com")).toBeInTheDocument();
    // The counts are the vault's answer, read for this account.
    expect(await within(table).findByText("30")).toBeInTheDocument();
    expect(within(table).getByText("12")).toBeInTheDocument();
    expect(within(table).getByText("2020-01-01")).toBeInTheDocument();
    expect(within(table).queryByText("Summary")).not.toBeInTheDocument();
    expect(listAccountIdentities).toHaveBeenCalledWith(expect.anything(), undefined);
  });

  it("shows the profile's identities with no numbers until the vault answers", () => {
    listAccountIdentities.mockReturnValue(new Promise(() => {}));
    render(<IdentitiesSection profile={profile} />);

    const table = screen.getByRole("grid", { name: "Identities" });
    expect(within(table).getByText("archer@example.com")).toBeInTheDocument();
    expect(within(table).queryByText("30")).not.toBeInTheDocument();
    const row = within(table).getAllByRole("row")[1];
    expect(within(row).getAllByText("—").length).toBeGreaterThanOrEqual(5);
  });

  it("speaks of the account holder when the owner is looking, and asks for that account", () => {
    render(<IdentitiesSection profile={profile} managedAccountId={101} />);
    expect(screen.getByRole("heading", { name: "Identities" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "The account holder's phone numbers and emails. Import uses them to determine which messages belong to them.",
      ),
    ).toBeInTheDocument();
    expect(listAccountIdentities).toHaveBeenCalledWith(expect.anything(), 101);
  });

  it("says so when there are no identities, and still offers to add one", () => {
    listAccountIdentities.mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 });
    render(<IdentitiesSection profile={{ ...profile, phones: [], emails: [] }} />);

    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
    expect(screen.getByText("No identities yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add identity" })).toBeEnabled();
  });

  it("adds an identity through the dialog and closes it when the vault has it", async () => {
    const user = userEvent.setup({ delay: null });
    mutateAsync.mockResolvedValue({ ...profile, emails: [...profile.emails, "new@example.com"] });
    render(<IdentitiesSection profile={profile} />);

    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add identity" }));
    const dialog = await screen.findByRole("dialog", { name: "Add identity" });
    await user.click(within(dialog).getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await user.type(within(dialog).getByRole("textbox", { name: "Identity" }), "new@example.com");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));

    expect(mutateAsync).toHaveBeenCalledWith({
      identities: [{ address: "new@example.com", service: "email" }],
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("keeps the dialog open and says why when the vault did not add the identity", async () => {
    const user = userEvent.setup({ delay: null });
    mutateAsync.mockResolvedValue(profile);
    render(<IdentitiesSection profile={profile} />);

    await user.click(screen.getByRole("button", { name: "Add identity" }));
    const dialog = await screen.findByRole("dialog", { name: "Add identity" });
    await user.type(within(dialog).getByRole("textbox", { name: "Identity" }), "+1 555 555 0199");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));

    expect(
      await within(dialog).findByText("The vault did not add that identity."),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Add identity" })).toBeInTheDocument();
  });

  it("asks before removing an identity, and removes it only on Remove", async () => {
    const user = userEvent.setup({ delay: null });
    mutateAsync.mockResolvedValue({ ...profile, phones: [] });
    render(<IdentitiesSection profile={profile} />);

    await screen.findByText("30");
    const remove = screen.getByRole("button", { name: "Remove +15555550100 (Text message)" });
    expect(remove.className).not.toMatch(/opacity-0/);
    await user.click(remove);
    const dialog = await screen.findByRole("dialog", { name: "Remove identity?" });
    expect(
      within(dialog).getByText(
        "12 direct messages and 30 group messages will no longer be associated with this account.",
      ),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Remove +15555550100 (Text message)" }));
    const again = await screen.findByRole("dialog", { name: "Remove identity?" });
    await user.click(within(again).getByRole("button", { name: "Remove" }));
    expect(mutateAsync).toHaveBeenCalledWith({
      remove_identities: [{ address: "+15555550100", service: "phone" }],
    });
  });
});

describe("removeBody", () => {
  it("names only the kind of message the identity has, singular when it is one", () => {
    expect(removeBody(identities[1])).toBe(
      "1 direct message will no longer be associated with this account.",
    );
    expect(removeBody(identities[0])).toBe(
      "12 direct messages and 30 group messages will no longer be associated with this account.",
    );
  });

  it("says so when the identity has no messages", () => {
    expect(removeBody(identities[2])).toBe(
      "archer@example.com has no messages. It will no longer count as this account's own.",
    );
  });
});
