/** @vitest-environment jsdom */

/**
 * The two real collections, driven through the real actions.
 *
 * `nameCollection.test.tsx` builds its own collection with `groupsOver()`, a
 * hand-written copy of the configuration in `contactGroups.ts`. That tests the
 * engine, and tests it well, but it means the configuration itself has no
 * test: changing `invalidates` in `contactGroups.ts` or `messageTags.ts` — the
 * lists that go stale after a write — failed nothing in the suite, and a
 * screen would quietly keep showing a group name that had just been renamed.
 * ADR-0002 makes those keys the whole mechanism by which the app learns that
 * something changed, so they are the part worth pinning.
 *
 * These import `contactGroups` and `messageTags` themselves. Only the vault
 * routes are faked, at the same boundary the rest of the suite uses.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { contactGroups } from "./contactGroups";
import { messageTags } from "./messageTags";
import { useNameCollectionActions } from "./nameCollection";
import * as vaultApi from "./vaultApi";
import { keys } from "./vaultKeys";

/** A cache key as the client sees it: the account, then the key itself. */
function scoped(key: readonly string[]): unknown[] {
  return ["vault", 7, ...key];
}

vi.mock("./auth", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

vi.mock("./vaultApi", () => ({
  listContactGroups: vi.fn().mockResolvedValue({ items: [] }),
  createContactGroup: vi.fn().mockResolvedValue({ id: 3, name: "Work" }),
  updateContactGroup: vi.fn().mockResolvedValue({ id: 12, name: "Fam" }),
  deleteContactGroup: vi.fn().mockResolvedValue(undefined),
  updateContactGroupMembers: vi.fn().mockResolvedValue({ added: 1, removed: 0 }),
  listMessageTags: vi.fn().mockResolvedValue({ items: [] }),
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

/** Every query key the collection marked stale during `run`. */
async function keysInvalidatedBy(run: () => Promise<unknown>): Promise<unknown[]> {
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await run();
  return invalidate.mock.calls.map((call) => call[0]?.queryKey);
}

describe("contact groups are wired to the lists that show a group name", () => {
  it("marks its own list and the contact lists stale after a create", async () => {
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const invalidated = await keysInvalidatedBy(() => result.current.create("Work"));

    expect(vi.mocked(vaultApi.createContactGroup)).toHaveBeenCalledWith({ name: "Work" });
    expect(invalidated).toEqual(
      expect.arrayContaining([scoped(keys.contactGroups.all), scoped(keys.contacts.all)]),
    );
  });

  it("marks the same lists stale after a rename, which is what changes on screen", async () => {
    client.setQueryData(scoped(keys.contactGroups.all), [{ id: 12, name: "Family" }]);
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const invalidated = await keysInvalidatedBy(() => result.current.rename("Family", "Fam"));

    expect(vi.mocked(vaultApi.updateContactGroup)).toHaveBeenCalledWith(12, { name: "Fam" });
    expect(invalidated).toEqual(
      expect.arrayContaining([scoped(keys.contactGroups.all), scoped(keys.contacts.all)]),
    );
  });

  it("marks the same lists stale after a delete", async () => {
    client.setQueryData(scoped(keys.contactGroups.all), [{ id: 12, name: "Family" }]);
    const { result } = renderHook(() => useNameCollectionActions(contactGroups), { wrapper });

    const invalidated = await keysInvalidatedBy(() => result.current.remove("Family"));

    expect(vi.mocked(vaultApi.deleteContactGroup)).toHaveBeenCalledWith(12);
    expect(invalidated).toEqual(
      expect.arrayContaining([scoped(keys.contactGroups.all), scoped(keys.contacts.all)]),
    );
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

    const invalidated = await keysInvalidatedBy(() => result.current.create("Receipts"));

    expect(vi.mocked(vaultApi.createMessageTag)).toHaveBeenCalledWith({ name: "Receipts" });
    expect(invalidated).toEqual(
      expect.arrayContaining([
        scoped(keys.messageTags.all),
        scoped(keys.conversations.all),
        scoped(keys.trash.all),
      ]),
    );
  });

  it("marks the same lists stale after a rename", async () => {
    client.setQueryData(scoped(keys.messageTags.all), [{ id: 14, name: "Bills" }]);
    const { result } = renderHook(() => useNameCollectionActions(messageTags), { wrapper });

    const invalidated = await keysInvalidatedBy(() => result.current.rename("Bills", "Utilities"));

    expect(invalidated).toEqual(
      expect.arrayContaining([
        scoped(keys.messageTags.all),
        scoped(keys.conversations.all),
        scoped(keys.trash.all),
      ]),
    );
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
    expect(contactGroups.routes.list).toBe(vaultApi.listContactGroups);
    expect(messageTags.routes.list).toBe(vaultApi.listMessageTags);
  });
});
