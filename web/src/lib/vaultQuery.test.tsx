/** @vitest-environment jsdom */

/**
 * What this module adds on top of TanStack Query, and nothing else.
 *
 * Caching, deduplication and refetching are the library's and are not retested
 * here. What is ours is the account prefix on every key, and the mapping from
 * the library's flags to the shape a long list renders from.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OffsetPage } from "./vaultQuery";
import { useVaultCache, useVaultPagedList, useVaultQuery } from "./vaultQuery";

const account = { current: 7 };
vi.mock("./auth", () => ({
  useAuth: () => ({ accountId: account.current }),
}));

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  account.current = 7;
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
});

describe("useVaultQuery", () => {
  it("names the cache entry after the signed-in account", async () => {
    const { result } = renderHook(() => useVaultQuery(["contact-groups"], async () => ["Family"]), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toEqual(["Family"]));
    expect(client.getQueryData(["vault", 7, "contact-groups"])).toEqual(["Family"]);
  });

  it("does not hand one account the entry another account filled", async () => {
    // A cache that keeps entries and treats them as fresh, which is the only
    // condition under which the old shape could serve the wrong account. With
    // entries collected on unmount the refetch happens anyway and the test
    // would pass whether or not the key names the account.
    client = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
          gcTime: Number.POSITIVE_INFINITY,
          staleTime: Number.POSITIVE_INFINITY,
        },
      },
    });

    const fetchGroups = vi.fn(async () => ["Family"]);
    const first = renderHook(() => useVaultQuery(["contact-groups"], fetchGroups), { wrapper });
    await waitFor(() => expect(first.result.current.data).toEqual(["Family"]));
    first.unmount();

    // A different account asks for the same thing.
    account.current = 8;
    fetchGroups.mockResolvedValue(["Work"]);
    const second = renderHook(() => useVaultQuery(["contact-groups"], fetchGroups), { wrapper });

    await waitFor(() => expect(second.result.current.data).toEqual(["Work"]));
    expect(second.result.current.data).not.toEqual(["Family"]);
    expect(fetchGroups).toHaveBeenCalledTimes(2);
  });

  it("reports the error rather than an empty result when the vault refuses", async () => {
    const { result } = renderHook(
      () =>
        useVaultQuery(["contact-groups"], async () => {
          throw new Error("nope");
        }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.error?.message).toBe("nope"));
    expect(result.current.data).toBeUndefined();
  });
});

/** A page of `count` numbered rows, out of `total`. */
function page(offset: number, count: number, total: number): OffsetPage<number> {
  return { items: Array.from({ length: count }, (_, i) => offset + i), total };
}

describe("useVaultPagedList", () => {
  it("flattens the pages loaded so far and reports the vault's total", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 5));
    const { result } = renderHook(
      () => useVaultPagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.items).toEqual([0, 1]));
    expect(result.current.total).toBe(5);
    expect(result.current.hasMore).toBe(true);

    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.items).toEqual([0, 1, 2, 3]));
  });

  it("has no next page once the loaded rows cover the total", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 2));
    const { result } = renderHook(
      () => useVaultPagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.items).toEqual([0, 1]));
    expect(result.current.hasMore).toBe(false);
  });

  it("asks for the first page size first and the fill size afterwards", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 3, 9));
    const { result } = renderHook(
      () => useVaultPagedList(["rows"], fetchPage, { firstPageSize: 3, fillPageSize: 7 }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.items).toHaveLength(3));
    act(() => result.current.loadMore());
    await waitFor(() => expect(fetchPage).toHaveBeenCalledTimes(2));

    expect(fetchPage.mock.calls[0]?.[0]).toMatchObject({ limit: 3, offset: 0 });
    expect(fetchPage.mock.calls[1]?.[0]).toMatchObject({ limit: 7, offset: 3 });
  });

  it("separates the first load from a later page: loading, then filling", async () => {
    let release: (() => void) | null = null;
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => {
      if (offset > 0) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return page(offset, 2, 6);
    });

    const { result } = renderHook(
      () => useVaultPagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );

    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.loadMore());
    // A later page is loading, and the rows already on screen stay put.
    await waitFor(() => expect(result.current.filling).toBe(true));
    expect(result.current.loading).toBe(false);
    expect(result.current.items).toEqual([0, 1]);

    act(() => release?.());
    await waitFor(() => expect(result.current.filling).toBe(false));
  });

  it("starts over when the key changes, rather than appending to the old list", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 4));
    const { result, rerender } = renderHook(
      ({ q }: { q: string }) =>
        useVaultPagedList(["rows", q], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper, initialProps: { q: "first" } },
    );
    await waitFor(() => expect(result.current.items).toEqual([0, 1]));
    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.items).toEqual([0, 1, 2, 3]));

    rerender({ q: "second" });
    await waitFor(() => expect(result.current.items).toEqual([0, 1]));
  });

  it("does not ask for another page while one is already loading", async () => {
    let release: (() => void) | null = null;
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => {
      if (offset > 0) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return page(offset, 2, 10);
    });
    const { result } = renderHook(
      () => useVaultPagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.items).toEqual([0, 1]));

    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.filling).toBe(true));
    act(() => result.current.loadMore());
    act(() => result.current.loadMore());

    expect(fetchPage).toHaveBeenCalledTimes(2);
    act(() => release?.());
    await waitFor(() => expect(result.current.filling).toBe(false));
  });
});

describe("useVaultCache", () => {
  it("reads and writes under the signed-in account's name", () => {
    const { result } = renderHook(() => useVaultCache(), { wrapper });
    act(() => {
      result.current.set(["contact-groups"], [{ id: 1, name: "Family" }]);
    });
    expect(client.getQueryData(["vault", 7, "contact-groups"])).toEqual([
      { id: 1, name: "Family" },
    ]);
    expect(result.current.read(["contact-groups"])).toEqual([{ id: 1, name: "Family" }]);

    // Another account's entry is not this account's to read.
    client.setQueryData(["vault", 8, "contact-groups"], [{ id: 9, name: "Work" }]);
    expect(result.current.read(["contact-groups"])).toEqual([{ id: 1, name: "Family" }]);
  });

  it("asks the vault and stores the answer under the account's key", async () => {
    const { result } = renderHook(() => useVaultCache(), { wrapper });
    await expect(result.current.fetch(["contact-groups"], async () => ["Family"])).resolves.toEqual(
      ["Family"],
    );
    expect(client.getQueryData(["vault", 7, "contact-groups"])).toEqual(["Family"]);
  });

  it("patches every entry under one prefix and puts them all back from a snapshot", () => {
    client.setQueryData(["vault", 7, "contacts", "list", ""], { total: 1 });
    client.setQueryData(["vault", 7, "contacts", "list", "ada"], { total: 2 });
    client.setQueryData(["vault", 7, "conversations", "list", ""], { total: 3 });
    const { result } = renderHook(() => useVaultCache(), { wrapper });

    const taken = result.current.snapshot(["contacts"]);
    expect(taken).toHaveLength(2);

    act(() => {
      result.current.patch<{ total: number }>(["contacts"], (entry) =>
        entry ? { total: entry.total + 10 } : entry,
      );
    });
    expect(client.getQueryData(["vault", 7, "contacts", "list", ""])).toEqual({
      total: 11,
    });
    expect(client.getQueryData(["vault", 7, "contacts", "list", "ada"])).toEqual({
      total: 12,
    });
    // A different resource under a different prefix is untouched.
    expect(client.getQueryData(["vault", 7, "conversations", "list", ""])).toEqual({
      total: 3,
    });

    act(() => {
      result.current.restore(taken);
    });
    expect(client.getQueryData(["vault", 7, "contacts", "list", ""])).toEqual({
      total: 1,
    });
    expect(client.getQueryData(["vault", 7, "contacts", "list", "ada"])).toEqual({
      total: 2,
    });
  });

  it("marks several prefixes stale in one call", async () => {
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useVaultCache(), { wrapper });
    await result.current.invalidate(["message-tags"], ["conversations"]);
    expect(invalidate.mock.calls.map((call) => call[0]?.queryKey)).toEqual([
      ["vault", 7, "message-tags"],
      ["vault", 7, "conversations"],
    ]);
  });
});
