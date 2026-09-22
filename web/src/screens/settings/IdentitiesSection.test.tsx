/** @vitest-environment jsdom */

import { cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProfile } from "../../lib/account";
import { mockedAuth, renderWithVault as render } from "../../test/vaultProviders";
import { IdentitiesSection } from "./IdentitiesSection";
import { removeBody, sortIdentities } from "./identities";

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

/** What the vault lists for `profile`, with the messages each identity takes part in. */
const identities = [
  { handle: "+15555550100", service: "phone", direct_messages: 12, group_messages: 30 },
  { handle: "bob@example.com", service: "email", direct_messages: 1, group_messages: 0 },
  { handle: "archer@example.com", service: "email", direct_messages: 0, group_messages: 0 },
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
  it("lists identities in a table saying whether each is in use, and no counts", async () => {
    render(<IdentitiesSection profile={profile} />);

    const table = screen.getByRole("grid", { name: "Identities" });
    expect(within(table).getByRole("columnheader", { name: /Type/ })).toBeInTheDocument();
    expect(within(table).getByRole("columnheader", { name: /^Identity/ })).toBeInTheDocument();
    expect(within(table).getByRole("columnheader", { name: /In use/ })).toBeInTheDocument();
    expect(within(table).queryByText(/messages/)).not.toBeInTheDocument();
    expect(within(table).getByText("Text message")).toBeInTheDocument();
    expect(within(table).getByText("bob@example.com")).toBeInTheDocument();
    // Whether an identity is in use is the vault's answer, read for this account.
    expect(await within(table).findAllByText("Yes")).toHaveLength(2);
    expect(within(table).getAllByText("No")).toHaveLength(1);
    expect(within(table).queryByText("30")).not.toBeInTheDocument();
    expect(listAccountIdentities).toHaveBeenCalledWith(expect.anything(), undefined);
  });

  it("asks the vault for the opened account's identities when the owner is looking", () => {
    render(<IdentitiesSection profile={profile} managedAccountId={101} />);
    expect(listAccountIdentities).toHaveBeenCalledWith(expect.anything(), 101);
  });

  it("sorts by a column when its header is clicked", async () => {
    const user = userEvent.setup({ delay: null });
    render(<IdentitiesSection profile={profile} />);

    const cells = () =>
      screen.getAllByRole("rowheader").map((cell) => cell.textContent?.trim() ?? "");
    expect(cells()).toEqual(["+15555550100", "bob@example.com", "archer@example.com"]);

    await user.click(screen.getByRole("columnheader", { name: /^Identity/ }));
    expect(cells()).toEqual(["+15555550100", "archer@example.com", "bob@example.com"]);

    await user.click(screen.getByRole("columnheader", { name: /^Identity/ }));
    expect(cells()).toEqual(["bob@example.com", "archer@example.com", "+15555550100"]);

    await waitFor(() => expect(screen.getAllByText("Yes")).toHaveLength(2));
    await user.click(screen.getByRole("columnheader", { name: /In use/ }));
    expect(cells()[0]).toBe("archer@example.com");
  });

  it("refuses a phone number or email address that is not one, before asking the vault", async () => {
    const user = userEvent.setup({ delay: null });
    render(<IdentitiesSection profile={profile} />);

    await user.type(screen.getByRole("textbox", { name: "New identity" }), "12");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByText("Enter a phone number like +1 555-123-4567.")).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Identity service/ }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await user.clear(screen.getByRole("textbox", { name: "New identity" }));
    await user.type(screen.getByRole("textbox", { name: "New identity" }), "not-an-address");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByText("Enter an email address like you@example.com.")).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("asks before removing an identity, and removes it only on Remove", async () => {
    const user = userEvent.setup({ delay: null });
    mutateAsync.mockResolvedValue({ ...profile, phones: [] });
    render(<IdentitiesSection profile={profile} />);

    await waitFor(() => expect(screen.getAllByText("Yes")).toHaveLength(2));
    await user.click(screen.getByRole("button", { name: "Remove +15555550100" }));
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

    await user.click(screen.getByRole("button", { name: "Remove +15555550100" }));
    const again = await screen.findByRole("dialog", { name: "Remove identity?" });
    await user.click(within(again).getByRole("button", { name: "Remove" }));
    expect(mutateAsync).toHaveBeenCalledWith({
      remove_handles: [{ handle: "+15555550100", service: "phone" }],
    });
  });

  it("keeps the type picker one fixed width and shows Remove only on its row's hover, at the right", () => {
    render(<IdentitiesSection profile={profile} />);

    const remove = screen.getByRole("button", { name: "Remove +15555550100" });
    const row = remove.closest("[role=row]");
    expect(row?.lastElementChild).toContainElement(remove);

    const picker = screen.getByRole("button", { name: /Identity service/ }).parentElement;
    expect(picker?.className).toContain("w-[10.5rem]");
    expect(screen.getByRole("button", { name: "Remove +15555550100" }).className).toContain(
      "group-hover:opacity-100",
    );
  });
});

describe("removeBody", () => {
  it("names only the kind of message the identity has, singular when it is one", () => {
    expect(
      removeBody({ handle: "a", service: "email", direct_messages: 1, group_messages: 0 }),
    ).toBe("1 direct message will no longer be associated with this account.");
    expect(
      removeBody({ handle: "a", service: "email", direct_messages: 0, group_messages: 2 }),
    ).toBe("2 group messages will no longer be associated with this account.");
  });

  it("says so when the identity takes part in no messages", () => {
    expect(
      removeBody({ handle: "a@b.co", service: "email", direct_messages: 0, group_messages: 0 }),
    ).toBe("a@b.co takes part in no messages. It will no longer count as this account's own.");
  });
});

describe("sortIdentities", () => {
  const rows = [
    { handle: "+15555550100", service: "phone", direct_messages: 3, group_messages: 0 },
    { handle: "bob@example.com", service: "email", direct_messages: 1, group_messages: 9 },
  ];

  it("leaves the vault's order alone with no sort", () => {
    expect(sortIdentities(rows, null)).toEqual(rows);
  });

  it("sorts by the type's label, so Email comes before Text message", () => {
    expect(sortIdentities(rows, { column: "service", direction: "ascending" })[0]?.service).toBe(
      "email",
    );
  });
});
