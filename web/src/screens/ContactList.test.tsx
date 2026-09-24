/** @vitest-environment jsdom */

/**
 * Regression test: the Groups menu's checkmarks used to come from a stale
 * snapshot once the checked contact's own group membership emptied the
 * checked set (unticking the only group on the only checked contact, on
 * that group's own page, drops the row out of the list). The menu fell back
 * to the pre-write snapshot and kept showing the group as ticked forever.
 * The fix resolves that fallback against the live contact list instead of
 * the stale rows, so a membership write the menu itself caused is reflected
 * right away.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RightPane from "../components/RightPane";
import { RightToolbarProvider } from "../components/RightToolbarContext";
import { mockedAuth, VaultProviders } from "../test/vaultProviders";
import ContactList from "./ContactList";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/vaultApi", () => ({
  listContacts: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
}));

import { listContactGroups, listContacts, updateContactGroupMembers } from "../lib/vaultApi";

const listContactsMock = vi.mocked(listContacts);
const listContactGroupsMock = vi.mocked(listContactGroups);
const updateMembersMock = vi.mocked(updateContactGroupMembers);

/** True once the vault has actually dropped Alice's Family membership. */
let familyRemoved = false;

beforeEach(() => {
  vi.clearAllMocks();
  familyRemoved = false;
  // Mirrors what the vault would answer: the write below flips this, and the
  // invalidate the mutation issues on settling refetches this same mock, so
  // the test also proves the optimistic patch and the server truth agree —
  // not just the moment right after the click.
  listContactsMock.mockImplementation(
    async () =>
      ({
        items: [
          {
            id: 1,
            name: "Alice",
            identity_count: 1,
            handles: [],
            groups: familyRemoved ? [] : ["Family"],
          },
        ],
        total: 1,
        limit: 200,
        offset: 0,
      }) as unknown as Awaited<ReturnType<typeof listContacts>>,
  );
  listContactGroupsMock.mockResolvedValue({
    items: [{ id: 10, name: "Family" }],
  } as unknown as Awaited<ReturnType<typeof listContactGroups>>);
});

afterEach(() => {
  cleanup();
});

describe("ContactList", () => {
  it("keeps the Groups menu's checkmark live after unticking the group the page is filtered to", async () => {
    updateMembersMock.mockImplementation(async () => {
      familyRemoved = true;
      return { added: 0, removed: 1 };
    });

    render(
      <VaultProviders>
        <RightToolbarProvider>
          <RightPane>
            <ContactList groupFilter="Family" onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </VaultProviders>,
    );

    const rowCheckbox = await screen.findByRole("checkbox", { name: "Select Alice" });
    act(() => {
      rowCheckbox.click();
    });
    await waitFor(() => expect(rowCheckbox).toBeChecked());

    const menuButton = await screen.findByRole("button", { name: "Contact Groups" });
    act(() => {
      menuButton.click();
    });

    const familyCheckbox = await screen.findByRole("checkbox", { name: "Family" });
    await waitFor(() => expect(familyCheckbox).toBeChecked());

    // Untick Family on the only checked contact, on the Family group page
    // itself: the row leaves the list, the checked set empties, and the menu
    // falls back to its last-known targets.
    act(() => {
      familyCheckbox.click();
    });

    await waitFor(() =>
      expect(updateMembersMock).toHaveBeenCalledWith(10, { add: [], remove: [1] }),
    );
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Family" })).not.toBeChecked());
  });

  it("checks every contact between a shift-click and the furthest checked contact", async () => {
    const names = ["Alice", "Bob", "Carol", "Dave", "Erin"];
    listContactsMock.mockResolvedValue({
      items: names.map((name, i) => ({
        id: i + 1,
        name,
        identity_count: 1,
        handles: [],
        groups: [],
      })),
      total: names.length,
      limit: 200,
      offset: 0,
    } as unknown as Awaited<ReturnType<typeof listContacts>>);

    render(
      <VaultProviders>
        <RightToolbarProvider>
          <RightPane>
            <ContactList onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </VaultProviders>,
    );

    const box = (name: string) => screen.getByRole("checkbox", { name: `Select ${name}` });
    const checkedNames = () => names.filter((name) => (box(name) as HTMLInputElement).checked);

    await screen.findByRole("checkbox", { name: "Select Erin" });
    fireEvent.click(box("Bob"));
    await waitFor(() => expect(checkedNames()).toEqual(["Bob"]));

    // Checks from the last clicked row to the shift-clicked one.
    fireEvent.click(box("Erin"), { shiftKey: true });
    await waitFor(() => expect(checkedNames()).toEqual(["Bob", "Carol", "Dave", "Erin"]));

    // Unchecks the same way: uncheck one end, shift-click the other.
    fireEvent.click(box("Dave"));
    fireEvent.click(box("Carol"), { shiftKey: true });
    await waitFor(() => expect(checkedNames()).toEqual(["Bob", "Erin"]));

    // The range starts at the last clicked row (Carol), not at the furthest
    // checked one (Erin), so Dave stays unchecked.
    fireEvent.click(box("Alice"), { shiftKey: true });
    await waitFor(() => expect(checkedNames()).toEqual(["Alice", "Bob", "Carol", "Erin"]));
  });
});
