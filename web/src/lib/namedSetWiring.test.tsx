/** @vitest-environment jsdom */

/**
 * The two real collections, driven through the real actions.
 *
 * `nameCollection.test.tsx` builds its own collection with `groupsOver()`, a
 * hand-written copy of the configuration in `contactGroups.ts`. That tests the
 * engine, and tests it well, but it means the configuration itself has no
 * test. Two parts of it are worth pinning: the lists that show a name are
 * stale after a write, so no screen keeps a group name that was just renamed,
 * and the chip targets decide where a ticked box shows before the server
 * answers.
 *
 * These import `contactGroups` and `messageTags` themselves. Only the server
 * routes are faked, at the same boundary the rest of the suite uses.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { freshEntries, seedEntries } from "../test/staleEntries";
import { contactGroups } from "./contactGroups";
import { messageTags } from "./messageTags";
import { useNameCollectionActions } from "./nameCollection";
import { keys } from "./queryKeys";
import * as serverApi from "./serverApi";

/** A cache key as the client sees it: the account, then the key itself. */
function scoped(key: readonly string[]): unknown[] {
  return ["server", 7, ...key];
}

/** The lists that show a Contact Group's name. */
const showGroups = [keys.contactGroups.all, keys.contacts.all];
/** The lists that show a Message Tag's name, and the Trash count a tag search narrows. */
const showTags = [keys.messageTags.all, keys.conversations.all, keys.trash.all];

vi.mock("./auth", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

vi.mock("./serverApi", () => ({
  listContactGroups: vi.fn().mockResolvedValue([]),
  createContactGroup: vi.fn().mockResolvedValue({ id: 3, name: "Work" }),
  updateContactGroup: vi.fn().mockResolvedValue({ id: 12, name: "Fam" }),
  deleteContactGroup: vi.fn().mockResolvedValue(undefined),
  updateContactGroupMembers: vi.fn().mockResolvedValue({ added: 1, removed: 0 }),
  listMessageTags: vi.fn().mockResolvedValue([]),
  createMessageTag: vi.fn().mockResolvedValue({ id: 4, name: "Receipts" }),
  updateMessageTag: vi.fn().mockResolvedValue({ id: 14, name: "Bills" }),
  deleteMessageTag: vi.fn().mockResolvedValue(undefined),
  updateMessageTagMembers: vi.fn().mockResolvedValue({ added: 1, removed: 0 }),
}));

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
});

/** Of the entries in `shown`, the ones still fresh after `run`. */
async function freshAfter(
  shown: readonly (readonly string[])[],
  run: () => Promise<unknown>,
): Promise<unknown[]> {
  seedEntries(client, 7, shown);
  await run();
  return freshEntries(client, 7, shown);
}

describe("contact groups are wired to the lists that show a group name", () => {
  it("marks its own list and the contact lists stale after a create", async () => {
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const fresh = await freshAfter(showGroups, () => result.current.create("Work"));

    expect(vi.mocked(serverApi.createContactGroup)).toHaveBeenCalledWith({ name: "Work" });
    expect(fresh).toEqual([]);
  });

  it("marks the same lists stale after a rename, which is what changes on screen", async () => {
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const fresh = await freshAfter(showGroups, () => {
      client.setQueryData(scoped(keys.contactGroups.all), [{ id: 12, name: "Family" }]);
      return result.current.rename("Family", "Fam");
    });

    expect(vi.mocked(serverApi.updateContactGroup)).toHaveBeenCalledWith(12, { name: "Fam" });
    expect(fresh).toEqual([]);
  });

  it("marks the same lists stale after a delete", async () => {
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const fresh = await freshAfter(showGroups, () => {
      client.setQueryData(scoped(keys.contactGroups.all), [{ id: 12, name: "Family" }]);
      return result.current.remove("Family");
    });

    expect(vi.mocked(serverApi.deleteContactGroup)).toHaveBeenCalledWith(12);
    expect(fresh).toEqual([]);
  });

  it("patches the chips on contact rows and on the open contact, both of them", () => {
    // The chip targets decide where a ticked box shows immediately, before the
    // refetch lands. A group shows on the contact list and in the drawer, so
    // dropping either target loses the tick on that surface alone — the kind
    // of thing no screen test notices, because they fake this module.
    expect(contactGroups.chips.map((chip) => chip.shape)).toEqual(["pages", "row"]);
    expect(contactGroups.chips.map((chip) => chip.field)).toEqual(["groups", "groups"]);
    expect(contactGroups.chips.map((chip) => chip.key)).toEqual([
      keys.contacts.lists,
      keys.contacts.details,
    ]);
  });
});

describe("message tags are wired to the lists that show a tag name", () => {
  it("marks its own list, the conversations and the trash count stale after a create", async () => {
    const { result } = renderHook(() => useNameCollectionActions(messageTags), { wrapper });

    const fresh = await freshAfter(showTags, () => result.current.create("Receipts"));

    expect(vi.mocked(serverApi.createMessageTag)).toHaveBeenCalledWith({ name: "Receipts" });
    expect(fresh).toEqual([]);
  });

  it("marks the same lists stale after a rename", async () => {
    const { result } = renderHook(() => useNameCollectionActions(messageTags), { wrapper });

    const fresh = await freshAfter(showTags, () => {
      client.setQueryData(scoped(keys.messageTags.all), [{ id: 14, name: "Bills" }]);
      return result.current.rename("Bills", "Utilities");
    });

    expect(fresh).toEqual([]);
  });

  it("patches the tag chips on conversation rows", () => {
    expect(messageTags.chips.map((chip) => chip.shape)).toEqual(["pages"]);
    expect(messageTags.chips.map((chip) => chip.field)).toEqual(["tags"]);
    expect(messageTags.chips.map((chip) => chip.key)).toEqual([keys.conversations.lists]);
  });
});

describe("the two collections stay distinct", () => {
  it("keeps separate keys, labels and route sets", () => {
    expect(contactGroups.key).not.toEqual(messageTags.key);
    expect(contactGroups.label).toBe("group");
    expect(messageTags.label).toBe("tag");
    expect(contactGroups.routes.list).toBe(serverApi.listContactGroups);
    expect(messageTags.routes.list).toBe(serverApi.listMessageTags);
  });
});
