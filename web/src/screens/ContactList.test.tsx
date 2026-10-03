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
import { groupListQuery } from "../lib/contactGroups";
import { mockedAuth, Providers } from "../test/providers";
import ContactList from "./ContactList";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", () => ({
  listContacts: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
  exportAddressBook: vi.fn(),
}));

vi.mock("../lib/saveTextFile", () => ({ saveTextFile: vi.fn().mockResolvedValue(true) }));

import { saveTextFile } from "../lib/saveTextFile";
import {
  exportAddressBook,
  listContactGroups,
  listContacts,
  updateContactGroupMembers,
} from "../lib/serverApi";

const listContactsMock = vi.mocked(listContacts);
const listContactGroupsMock = vi.mocked(listContactGroups);
const updateMembersMock = vi.mocked(updateContactGroupMembers);
const exportMock = vi.mocked(exportAddressBook);

/** True once the server has actually dropped Alice's Family membership. */
let familyRemoved = false;

beforeEach(() => {
  vi.clearAllMocks();
  familyRemoved = false;
  // Mirrors what the server would answer: the write below flips this, and the
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
            addresses: [],
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
      <Providers>
        <RightToolbarProvider>
          <RightPane>
            <ContactList groupFilter="Family" onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </Providers>,
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

  it("exports the group the list shows, or the checked rows when there are any", async () => {
    exportMock.mockResolvedValue("contact_id,display_name,groups,service,identity_type,identity\n");
    render(
      <Providers>
        <RightToolbarProvider>
          <RightPane>
            <ContactList groupFilter="Family" onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </Providers>,
    );
    const rowCheckbox = await screen.findByRole("checkbox", { name: "Select Alice" });

    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(saveTextFile).toHaveBeenCalledTimes(1));
    // The same search the list sends the server for this group.
    expect(exportMock).toHaveBeenLastCalledWith({ q: groupListQuery("Family", "") });

    fireEvent.click(rowCheckbox);
    await waitFor(() => expect(rowCheckbox).toBeChecked());
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(saveTextFile).toHaveBeenCalledTimes(2));
    expect(exportMock).toHaveBeenLastCalledWith({ ids: [1] });
  });

  it("checks every contact between a shift-click and the last clicked contact", async () => {
    const names = ["Alice", "Bob", "Carol", "Dave", "Erin"];
    listContactsMock.mockResolvedValue({
      items: names.map((name, i) => ({
        id: i + 1,
        name,
        identity_count: 1,
        addresses: [],
        groups: [],
      })),
      total: names.length,
      limit: 200,
      offset: 0,
    } as unknown as Awaited<ReturnType<typeof listContacts>>);

    render(
      <Providers>
        <RightToolbarProvider>
          <RightPane>
            <ContactList onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </Providers>,
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

  it("lists the server's Unknown and No group contacts once the full list is loaded", async () => {
    // Bob and Dave are Unknown; Dave also has a stored group. Alice is the
    // only contact that is neither Unknown nor in a stored group.
    const rows = [
      { id: 1, name: "Alice", groups: [], unknown: false },
      { id: 2, name: "Bob", groups: [], unknown: true },
      { id: 3, name: "Carol", groups: ["Family"], unknown: false },
      { id: 4, name: "Dave", groups: ["Family"], unknown: true },
    ];
    // Answers the way the server does, so the list is right whether the
    // browser filters the loaded rows or asks again.
    listContactsMock.mockImplementation(async ({ q } = {}) => {
      const items = rows
        .filter((row) => {
          if (q === "group:unknown") return row.unknown;
          if (q === "group:none") return row.groups.length === 0 && !row.unknown;
          return true;
        })
        .map((row) => ({ ...row, identity_count: 1, addresses: [] }));
      return { items, total: items.length, limit: 200, offset: 0 } as unknown as Awaited<
        ReturnType<typeof listContacts>
      >;
    });

    const page = (groupFilter: string | null) => (
      <Providers>
        <RightToolbarProvider>
          <RightPane>
            <ContactList groupFilter={groupFilter} onSelect={() => {}} />
          </RightPane>
        </RightToolbarProvider>
      </Providers>
    );
    const listed = () =>
      rows
        .map((row) => row.name)
        .filter((name) => screen.queryByRole("checkbox", { name: `Select ${name}` }) !== null);

    const { rerender } = render(page(null));
    await waitFor(() => expect(listed()).toEqual(["Alice", "Bob", "Carol", "Dave"]));

    rerender(page("unknown"));
    await waitFor(() => expect(listed()).toEqual(["Bob", "Dave"]));

    rerender(page("none"));
    await waitFor(() => expect(listed()).toEqual(["Alice"]));
  });
});
