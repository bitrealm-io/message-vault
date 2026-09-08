/** @vitest-environment jsdom */

/**
 * The name-based interface over id-addressed routes.
 *
 * Screens hold names; the vault addresses a set by id. Everything about that
 * translation — where the id comes from, what happens when the name is not
 * there, and which lists go stale after a write — is this module's, so it is
 * tested here at the interface with the routes faked by name.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import {
  type ChipTarget,
  createNameCollection,
  type MembersChanged,
  type NameCollectionRoutes,
  patchChips,
  useNameCollection,
  useNameCollectionActions,
  useSetNamedSetMembers,
  withName,
} from "./nameCollection";
import { keys } from "./vaultKeys";

vi.mock("./auth", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function fakeRoutes(): { [K in keyof NameCollectionRoutes]: Mock<NameCollectionRoutes[K]> } {
  return {
    list: vi.fn().mockResolvedValue({ items: [] }),
    create: vi.fn(),
    update: vi.fn(),
    remove: vi.fn().mockResolvedValue(undefined),
    updateMembers: vi.fn().mockResolvedValue({ added: 0, removed: 0 }),
  };
}

function groupsOver(routes: NameCollectionRoutes) {
  return createNameCollection({
    routes,
    key: keys.contactGroups.all,
    invalidates: [keys.contacts.all],
    chips: [
      { key: keys.contacts.lists, field: "groups", shape: "pages" },
      { key: keys.contacts.details, field: "groups", shape: "row" },
    ],
    label: "group",
    forName: (name) => `group:${name}`,
    reservedNames: new Set(["trash"]),
    reservedError: (name) => `${name} is reserved`,
  });
}

const KEY = ["vault", 7, "contact-groups"];
const PAGE_KEY = ["vault", 7, "contacts", "list", ""];
const DETAIL_KEY = ["vault", 7, "contacts", "detail", "1"];

/** A contact list page and an open contact, as the two queries would hold them. */
function seedContacts(): void {
  client.setQueryData(PAGE_KEY, {
    pages: [
      {
        items: [
          { id: "1", name: "Ada", groups: [] },
          { id: "2", name: "Ben", groups: ["Work"] },
        ],
        total: 2,
      },
    ],
    pageParams: [0],
  });
  client.setQueryData(DETAIL_KEY, { id: 1, name: "Ada", groups: [] });
}

/** Group chips on one row of the seeded page. */
function pageGroups(id: string): string[] | undefined {
  const entry = client.getQueryData<{ pages: { items: { id: string; groups: string[] }[] }[] }>(
    PAGE_KEY,
  );
  return entry?.pages[0].items.find((row) => row.id === id)?.groups;
}

/** Group chips on the open contact. */
function detailGroups(): string[] | undefined {
  return client.getQueryData<{ groups: string[] }>(DETAIL_KEY)?.groups;
}

/** A promise this test resolves when it chooses, so it can look mid-write. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  client = new QueryClient({
    // Not 0: the chip targets below are patched but never observed by a
    // `useQuery` in this file, and with `gcTime: 0` the library's own
    // garbage collector removes an unobserved entry on a real timer shortly
    // after it is created — a race that can fire while `waitFor` is still
    // polling for the optimistic chip, independent of anything the mutation
    // does. A hook that does observe (rendered elsewhere) is unaffected by
    // this either way.
    defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: 0 } },
  });
});

describe("useNameCollection", () => {
  it("answers the names in the vault's order", async () => {
    const routes = fakeRoutes();
    routes.list.mockResolvedValue({
      items: [
        { id: 2, name: "Family" },
        { id: 1, name: "Work" },
      ],
    });
    const { result } = renderHook(() => useNameCollection(groupsOver(routes)), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.names).toEqual(["Family", "Work"]);
  });
});

describe("useNameCollectionActions", () => {
  it("renames by the id the cache holds and invalidates the lists that show the name", async () => {
    const routes = fakeRoutes();
    routes.update.mockResolvedValue({ id: 12, name: "Fam" });
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.rename("Family", "Fam")).resolves.toBe("Fam");

    expect(routes.update).toHaveBeenCalledWith(12, { name: "Fam" });
    expect(routes.list).not.toHaveBeenCalled();
    const invalidated = invalidate.mock.calls.map((call) => call[0]?.queryKey);
    expect(invalidated).toEqual(
      expect.arrayContaining([
        ["vault", 7, "contact-groups"],
        ["vault", 7, "contacts"],
      ]),
    );
  });

  it("matches a name without regard to letter case", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await result.current.remove("family");
    expect(routes.remove).toHaveBeenCalledWith(12);
  });

  it("asks the vault once when the cache does not hold the name", async () => {
    const routes = fakeRoutes();
    routes.list.mockResolvedValue({ items: [{ id: 7, name: "Holiday" }] });
    routes.updateMembers.mockResolvedValue({ added: 2, removed: 0 });
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });

    await expect(result.current.setMembers("Holiday", { add: [1, 2] })).resolves.toEqual({
      added: 2,
      removed: 0,
    });
    expect(routes.list).toHaveBeenCalledTimes(1);
    expect(routes.updateMembers).toHaveBeenCalledWith(7, { add: [1, 2], remove: [] });
    expect(client.getQueryData(KEY)).toEqual([{ id: 7, name: "Holiday" }]);
  });

  it("throws without a request when the vault has no set of that name", async () => {
    const routes = fakeRoutes();
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.remove("Nope")).rejects.toThrow("group not found");
    expect(routes.list).toHaveBeenCalledTimes(1);
    expect(routes.remove).not.toHaveBeenCalled();
  });

  it("refuses a reserved name before asking the vault", async () => {
    const routes = fakeRoutes();
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.create("Trash")).rejects.toThrow("Trash is reserved");
    expect(routes.create).not.toHaveBeenCalled();
  });

  it("answers the created name and invalidates its own list", async () => {
    const routes = fakeRoutes();
    routes.create.mockResolvedValue({ id: 3, name: "Work" });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.create(" Work ")).resolves.toBe("Work");
    expect(routes.create).toHaveBeenCalledWith({ name: "Work" });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: KEY });
  });

  it("reports a write in flight, so a screen needs no busy flag of its own", async () => {
    const routes = fakeRoutes();
    const answer = deferred<{ id: number; name: string }>();
    routes.create.mockReturnValue(answer.promise);

    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    expect(result.current.pending).toBe(false);

    let write: Promise<string> = Promise.resolve("");
    act(() => {
      write = result.current.create("Work");
    });
    await waitFor(() => expect(result.current.pending).toBe(true));

    answer.resolve({ id: 3, name: "Work" });
    await write;
    await waitFor(() => expect(result.current.pending).toBe(false));
  });

  it("reports the failure a write ended in", async () => {
    const routes = fakeRoutes();
    routes.create.mockRejectedValue(new Error("vault said no"));
    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.create("Work")).rejects.toThrow("vault said no");
    await waitFor(() => expect(result.current.error?.message).toBe("vault said no"));
  });

  it("clears an earlier failure once a later write succeeds", async () => {
    const routes = fakeRoutes();
    routes.create.mockRejectedValue(new Error("create failed"));
    routes.update.mockResolvedValue({ id: 12, name: "Fam" });
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);

    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.create("Work")).rejects.toThrow("create failed");
    await waitFor(() => expect(result.current.error?.message).toBe("create failed"));

    await expect(result.current.rename("Family", "Fam")).resolves.toBe("Fam");
    await waitFor(() => expect(result.current.error).toBeNull());
  });

  it("reports the newer of two failures, not the first one to happen", async () => {
    const routes = fakeRoutes();
    routes.create.mockRejectedValue(new Error("create failed"));
    routes.update.mockRejectedValue(new Error("rename failed"));
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);

    const { result } = renderHook(() => useNameCollectionActions(groupsOver(routes)), { wrapper });
    await expect(result.current.create("Work")).rejects.toThrow("create failed");
    await waitFor(() => expect(result.current.error?.message).toBe("create failed"));

    await expect(result.current.rename("Family", "Fam")).rejects.toThrow("rename failed");
    await waitFor(() => expect(result.current.error?.message).toBe("rename failed"));
  });
});

describe("useSetNamedSetMembers", () => {
  it("draws the chips on the list and the open contact before the vault answers", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    seedContacts();
    const answer = deferred<MembersChanged>();
    routes.updateMembers.mockReturnValue(answer.promise);

    const { result } = renderHook(() => useSetNamedSetMembers(groupsOver(routes)), { wrapper });
    let write: Promise<MembersChanged> = Promise.resolve({ added: 0, removed: 0 });
    act(() => {
      write = result.current.mutateAsync({ name: "Family", patch: { add: [1] } });
    });

    await waitFor(() => expect(pageGroups("1")).toEqual(["Family"]));
    expect(detailGroups()).toEqual(["Family"]);
    expect(pageGroups("2")).toEqual(["Work"]);

    answer.resolve({ added: 1, removed: 0 });
    await write;
    expect(routes.updateMembers).toHaveBeenCalledWith(12, { add: [1], remove: [] });
  });

  it("takes a name off the rows it was removed from", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 5, name: "Work" }]);
    seedContacts();
    const { result } = renderHook(() => useSetNamedSetMembers(groupsOver(routes)), { wrapper });
    await result.current.mutateAsync({ name: "Work", patch: { remove: [2] } });
    expect(pageGroups("2")).toEqual([]);
    expect(routes.updateMembers).toHaveBeenCalledWith(5, { add: [], remove: [2] });
  });

  it("puts every row back when the vault refuses", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    seedContacts();
    routes.updateMembers.mockRejectedValue(new Error("nope"));

    const { result } = renderHook(() => useSetNamedSetMembers(groupsOver(routes)), { wrapper });
    await expect(
      result.current.mutateAsync({ name: "Family", patch: { add: [1] } }),
    ).rejects.toThrow("nope");

    expect(pageGroups("1")).toEqual([]);
    expect(detailGroups()).toEqual([]);
  });

  it("marks the group list and every contact stale once it settles", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useSetNamedSetMembers(groupsOver(routes)), { wrapper });
    await result.current.mutateAsync({ name: "Family", patch: { add: [1] } });
    expect(invalidate.mock.calls.map((call) => call[0]?.queryKey)).toEqual([
      ["vault", 7, "contact-groups"],
      ["vault", 7, "contacts"],
    ]);
  });

  it("cancels in-flight fetches before it patches, so no answer lands on top of the chip", async () => {
    const routes = fakeRoutes();
    client.setQueryData(KEY, [{ id: 12, name: "Family" }]);
    seedContacts();
    const order: string[] = [];
    const originalCancel = client.cancelQueries.bind(client);
    const originalSetQueriesData = client.setQueriesData.bind(client);
    vi.spyOn(client, "cancelQueries").mockImplementation((...args) => {
      order.push("cancel");
      return originalCancel(...args);
    });
    vi.spyOn(client, "setQueriesData").mockImplementation((...args) => {
      order.push("patch");
      return originalSetQueriesData(...args);
    });

    const { result } = renderHook(() => useSetNamedSetMembers(groupsOver(routes)), { wrapper });
    await result.current.mutateAsync({ name: "Family", patch: { add: [1] } });

    expect(order[0]).toBe("cancel");
    expect(order).toContain("patch");
    expect(order.indexOf("cancel")).toBeLessThan(order.indexOf("patch"));
  });
});

describe("withName", () => {
  it("adds a name that is not there under any spelling", () => {
    expect(withName(["Work"], "Family", true)).toEqual(["Work", "Family"]);
  });

  it("does nothing when the name is already there under another spelling", () => {
    expect(withName(["family"], "Family", true)).toEqual(["family"]);
  });

  it("removes a name regardless of letter case", () => {
    expect(withName(["Family", "Work"], "family", false)).toEqual(["Work"]);
  });
});

describe("patchChips", () => {
  const rowTarget: ChipTarget = { key: ["contacts", "detail"], field: "groups", shape: "row" };
  const pagesTarget: ChipTarget = { key: ["contacts", "list"], field: "groups", shape: "pages" };

  it("leaves a non-pages entry alone when the target expects pages", () => {
    const entry = { id: "1", groups: [] };
    expect(patchChips(entry, pagesTarget, new Set(["1"]), "Family", true)).toBe(entry);
  });

  it("treats a row with no groups field as starting from empty", () => {
    const entry = { id: "1", name: "Ada" };
    expect(patchChips(entry, rowTarget, new Set(["1"]), "Family", true)).toEqual({
      id: "1",
      name: "Ada",
      groups: ["Family"],
    });
  });
});
