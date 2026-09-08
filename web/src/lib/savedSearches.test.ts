/** @vitest-environment jsdom */

/**
 * What this module still owns.
 *
 * The cases that used to be here for the module-level cache, the shared
 * in-flight request, and the browser event announcing a change are gone with
 * the code they covered — that is TanStack Query's job now, and
 * `vaultQuery.test.tsx` covers the part of it that is ours. What remains is
 * this module's own behaviour: the shape it reads out of a response, the ids it
 * addresses mutations by, and putting a mutation's answer where the sidebar
 * reads it.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { type SavedSearch, useSavedSearchActions, useSavedSearches } from "./savedSearches";
import {
  createSavedSearch as createVaultSavedSearch,
  deleteSavedSearch as deleteVaultSavedSearch,
  listSavedSearches,
  updateSavedSearch as updateVaultSavedSearch,
} from "./vaultApi";

const account = { current: 7 };
vi.mock("./auth", () => ({ useAuth: () => ({ accountId: account.current }) }));

vi.mock("./vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./vaultApi")>()),
  listSavedSearches: vi.fn(),
  createSavedSearch: vi.fn(),
  updateSavedSearch: vi.fn(),
  deleteSavedSearch: vi.fn(),
}));

const list = vi.mocked(listSavedSearches);
const create = vi.mocked(createVaultSavedSearch);
const update = vi.mocked(updateVaultSavedSearch);
const remove = vi.mocked(deleteVaultSavedSearch);

function search(id: number, name: string, kind = "manual"): SavedSearch {
  return { id, name, query: `kind:group ${name}`, kind };
}

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return createElement(QueryClientProvider, { client }, children);
}

beforeEach(() => {
  vi.clearAllMocks();
  account.current = 7;
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
});

describe("useSavedSearches", () => {
  it("reads the list from the vault, not from browser storage", async () => {
    list.mockResolvedValue({ items: [search(1, "Family")], total: 1, limit: 40, offset: 0 });
    const { result } = renderHook(() => useSavedSearches(), { wrapper });
    await waitFor(() => expect(result.current.savedSearches).toEqual([search(1, "Family")]));
    expect(list).toHaveBeenCalled();
  });

  /**
   * The bug this whole change exists for.
   *
   * Saved searches used to be held in a module-level variable that `auth.tsx`
   * had to clear by hand on sign-in and sign-out. Both of its clearing lists
   * named four other caches and omitted this one, so signing in as a second
   * account showed the first account's saved searches until something else
   * refreshed them. The entry is now named with the account, so the second
   * account asks for something that was never written.
   */
  it("does not show one account the saved searches of another", async () => {
    // Entries are kept and treated as fresh here, so a key that did not name
    // the account would serve the first account's list to the second.
    client = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
          gcTime: Number.POSITIVE_INFINITY,
          staleTime: Number.POSITIVE_INFINITY,
        },
      },
    });

    list.mockResolvedValue({
      items: [search(1, "Alice's Family")],
      total: 1,
      limit: 40,
      offset: 0,
    });
    const first = renderHook(() => useSavedSearches(), { wrapper });
    await waitFor(() =>
      expect(first.result.current.savedSearches).toEqual([search(1, "Alice's Family")]),
    );
    first.unmount();

    account.current = 8;
    list.mockResolvedValue({ items: [search(2, "Bob's Work")], total: 1, limit: 40, offset: 0 });
    const second = renderHook(() => useSavedSearches(), { wrapper });

    await waitFor(() =>
      expect(second.result.current.savedSearches).toEqual([search(2, "Bob's Work")]),
    );
    expect(second.result.current.savedSearches).not.toContainEqual(search(1, "Alice's Family"));
  });

  it("keeps the kind the vault reports, so import rows stay identifiable", async () => {
    list.mockResolvedValue({
      items: [search(2, "Backup 1", "import")],
      total: 1,
      limit: 40,
      offset: 0,
    });
    const { result } = renderHook(() => useSavedSearches(), { wrapper });
    await waitFor(() => expect(result.current.savedSearches[0]?.kind).toBe("import"));
  });
});

describe("useSavedSearchActions", () => {
  it("addresses an update by id, sending both fields", async () => {
    update.mockResolvedValue(search(3, "Renamed"));
    const { result } = renderHook(() => useSavedSearchActions(), { wrapper });
    await result.current.update(3, "Renamed", "kind:direct");
    expect(update).toHaveBeenCalledWith(3, { name: "Renamed", query: "kind:direct" });
  });

  it("addresses a delete by id", async () => {
    remove.mockResolvedValue(undefined);
    const { result } = renderHook(() => useSavedSearchActions(), { wrapper });
    await result.current.remove(7);
    expect(remove).toHaveBeenCalledWith(7);
  });

  it("re-reads the list after a create", async () => {
    list
      .mockResolvedValueOnce({ items: [], total: 0, limit: 40, offset: 0 })
      .mockResolvedValueOnce({ items: [search(3, "Work")], total: 1, limit: 40, offset: 0 });
    create.mockResolvedValue(search(3, "Work"));
    const { result } = renderHook(
      () => ({ list: useSavedSearches(), actions: useSavedSearchActions() }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([]));
    await act(() => result.current.actions.create("Work", "kind:group Work"));
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([search(3, "Work")]));
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("re-reads the list after an update", async () => {
    list
      .mockResolvedValueOnce({ items: [search(3, "Work")], total: 1, limit: 40, offset: 0 })
      .mockResolvedValueOnce({ items: [search(3, "Renamed")], total: 1, limit: 40, offset: 0 });
    update.mockResolvedValue(search(3, "Renamed"));
    const { result } = renderHook(
      () => ({ list: useSavedSearches(), actions: useSavedSearchActions() }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([search(3, "Work")]));
    await act(() => result.current.actions.update(3, "Renamed", "kind:group Work"));
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([search(3, "Renamed")]));
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("re-reads the list after a delete", async () => {
    list
      .mockResolvedValueOnce({ items: [search(3, "Work")], total: 1, limit: 40, offset: 0 })
      .mockResolvedValueOnce({ items: [], total: 0, limit: 40, offset: 0 });
    remove.mockResolvedValue(undefined);
    const { result } = renderHook(
      () => ({ list: useSavedSearches(), actions: useSavedSearchActions() }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([search(3, "Work")]));
    await act(() => result.current.actions.remove(3));
    await waitFor(() => expect(result.current.list.savedSearches).toEqual([]));
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("reports a write in flight and the failure it ended in", async () => {
    let refuse: (reason: Error) => void = () => {};
    create.mockReturnValue(
      new Promise((_resolve, reject) => {
        refuse = reject;
      }),
    );
    const { result } = renderHook(() => useSavedSearchActions(), { wrapper });
    expect(result.current.pending).toBe(false);

    const write = result.current.create("Family", "kind:group");
    await waitFor(() => expect(result.current.pending).toBe(true));

    refuse(new Error("vault said no"));
    await expect(write).rejects.toThrow("vault said no");
    await waitFor(() => expect(result.current.error?.message).toBe("vault said no"));
    expect(result.current.pending).toBe(false);
  });

  it("keeps the same create function across a write, so an effect watching it does not re-run", async () => {
    create.mockResolvedValue(search(1, "Family"));
    const { result } = renderHook(() => useSavedSearchActions(), { wrapper });
    const before = result.current.create;

    await result.current.create("Family", "kind:group");
    await waitFor(() => expect(result.current.pending).toBe(false));

    expect(result.current.create).toBe(before);
  });
});
