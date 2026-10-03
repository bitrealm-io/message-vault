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
import { freshEntries, seedEntries } from "../test/staleEntries";
import type { OffsetPage } from "./routeQuery";
import { useRouteCache, useRoutePagedList, useRouteQuery } from "./routeQuery";

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

describe("useRouteQuery", () => {
  it("names the cache entry after the logged-in account", async () => {
    const { result } = renderHook(() => useRouteQuery(["contact-groups"], async () => ["Family"]), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toEqual(["Family"]));
    expect(client.getQueryData(["server", 7, "contact-groups"])).toEqual(["Family"]);
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
    const first = renderHook(() => useRouteQuery(["contact-groups"], fetchGroups), { wrapper });
    await waitFor(() => expect(first.result.current.data).toEqual(["Family"]));
    first.unmount();

    // A different account asks for the same thing.
    account.current = 8;
    fetchGroups.mockResolvedValue(["Work"]);
    const second = renderHook(() => useRouteQuery(["contact-groups"], fetchGroups), { wrapper });

    await waitFor(() => expect(second.result.current.data).toEqual(["Work"]));
    expect(second.result.current.data).not.toEqual(["Family"]);
    expect(fetchGroups).toHaveBeenCalledTimes(2);
  });

  it("reports the error rather than an empty result when the server refuses", async () => {
    const { result } = renderHook(
      () =>
        useRouteQuery(["contact-groups"], async () => {
          throw new Error("nope");
        }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.error?.message).toBe("nope"));
    expect(result.current.data).toBeUndefined();
  });
});

type Row = { id: number };

/** A page of `count` rows numbered from `offset`, out of `total`. */
function page(offset: number, count: number, total: number): OffsetPage<Row> {
  return { items: Array.from({ length: count }, (_, i) => ({ id: offset + i })), total };
}

/** The ids of the rows on screen, in order. */
function ids(rows: Row[]): number[] {
  return rows.map((row) => row.id);
}

describe("useRoutePagedList", () => {
  it("flattens the pages loaded so far and reports the server's total", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 5));
    const { result } = renderHook(
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );

    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));
    expect(result.current.total).toBe(5);
    expect(result.current.hasMore).toBe(true);

    act(() => result.current.loadMore());
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1, 2, 3]));
  });

  it("has no next page once the loaded rows cover the total", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 2));
    const { result } = renderHook(
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));
    expect(result.current.hasMore).toBe(false);
  });

  it("stops at the server's offset ceiling, however many rows the total names", async () => {
    // A browse list refuses an `offset` past 50,000 (`docs/architecture/http-api.md`,
    // "Lists"), so the list ends where the next page would start past it.
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 100));
    const { result } = renderHook(
      () =>
        useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2, maxOffset: 3 }),
      { wrapper },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));
    act(() => result.current.loadMore());
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1, 2, 3]));
    expect(result.current.total).toBe(100);
    expect(result.current.hasMore).toBe(false);
    expect(fetchPage).toHaveBeenCalledTimes(2);
  });

  it("asks for the first page size first and the fill size afterwards", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 3, 9));
    const { result } = renderHook(
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 3, fillPageSize: 7 }),
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
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );

    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.loadMore());
    // A later page is loading, and the rows already on screen stay put.
    await waitFor(() => expect(result.current.filling).toBe(true));
    expect(result.current.loading).toBe(false);
    expect(ids(result.current.items)).toEqual([0, 1]);

    act(() => release?.());
    await waitFor(() => expect(result.current.filling).toBe(false));
  });

  it("starts over when the key changes, rather than appending to the old list", async () => {
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, 4));
    const { result, rerender } = renderHook(
      ({ q }: { q: string }) =>
        useRoutePagedList(["rows", q], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper, initialProps: { q: "first" } },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));
    act(() => result.current.loadMore());
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1, 2, 3]));

    rerender({ q: "second" });
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));
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
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));

    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.filling).toBe(true));
    act(() => result.current.loadMore());
    act(() => result.current.loadMore());

    expect(fetchPage).toHaveBeenCalledTimes(2);
    act(() => release?.());
    await waitFor(() => expect(result.current.filling).toBe(false));
  });

  it("shows a row once when the next page repeats it", async () => {
    // A row was added before the end of the first page and another removed
    // after it, so the total stayed the same and the offsets moved by one: the
    // second page starts with the last row of the first.
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) =>
      offset === 0 ? page(0, 2, 5) : page(1, 2, 5),
    );
    const { result } = renderHook(
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));

    act(() => result.current.loadMore());
    await waitFor(() => expect(fetchPage).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.filling).toBe(false));
    expect(ids(result.current.items)).toEqual([0, 1, 2]);
  });

  it("fetches the list again from the start when a later page reports another total", async () => {
    // A row was added somewhere while the first page was on screen: the second
    // page counts one more row than the first did, so its offset is no longer
    // where the first page ended.
    let total = 5;
    const fetchPage = vi.fn(async ({ offset }: { offset: number }) => page(offset, 2, total));
    const { result } = renderHook(
      () => useRoutePagedList(["rows"], fetchPage, { firstPageSize: 2, fillPageSize: 2 }),
      { wrapper },
    );
    await waitFor(() => expect(ids(result.current.items)).toEqual([0, 1]));

    total = 6;
    act(() => result.current.loadMore());

    await waitFor(() => expect(fetchPage).toHaveBeenCalledTimes(4));
    expect(fetchPage.mock.calls.map(([args]) => args.offset)).toEqual([0, 2, 0, 2]);
    await waitFor(() => expect(result.current.refreshing).toBe(false));
    expect(ids(result.current.items)).toEqual([0, 1, 2, 3]);
    expect(result.current.total).toBe(6);
  });
});

describe("useRouteCache", () => {
  it("reads and writes under the logged-in account's name", () => {
    const { result } = renderHook(() => useRouteCache(), { wrapper });
    act(() => {
      result.current.set(["contact-groups"], [{ id: 1, name: "Family" }]);
    });
    expect(client.getQueryData(["server", 7, "contact-groups"])).toEqual([
      { id: 1, name: "Family" },
    ]);
    expect(result.current.read(["contact-groups"])).toEqual([{ id: 1, name: "Family" }]);

    // Another account's entry is not this account's to read.
    client.setQueryData(["server", 8, "contact-groups"], [{ id: 9, name: "Work" }]);
    expect(result.current.read(["contact-groups"])).toEqual([{ id: 1, name: "Family" }]);
  });

  it("asks the server and stores the answer under the account's key", async () => {
    const { result } = renderHook(() => useRouteCache(), { wrapper });
    await expect(result.current.fetch(["contact-groups"], async () => ["Family"])).resolves.toEqual(
      ["Family"],
    );
    expect(client.getQueryData(["server", 7, "contact-groups"])).toEqual(["Family"]);
  });

  it("patches every entry under one prefix and puts them all back from a snapshot", () => {
    client.setQueryData(["server", 7, "contacts", "list", ""], { total: 1 });
    client.setQueryData(["server", 7, "contacts", "list", "ada"], { total: 2 });
    client.setQueryData(["server", 7, "conversations", "list", ""], { total: 3 });
    const { result } = renderHook(() => useRouteCache(), { wrapper });

    const taken = result.current.snapshot(["contacts"]);
    expect(taken).toHaveLength(2);

    act(() => {
      result.current.patch<{ total: number }>(["contacts"], (entry) =>
        entry ? { total: entry.total + 10 } : entry,
      );
    });
    expect(client.getQueryData(["server", 7, "contacts", "list", ""])).toEqual({
      total: 11,
    });
    expect(client.getQueryData(["server", 7, "contacts", "list", "ada"])).toEqual({
      total: 12,
    });
    // A different resource under a different prefix is untouched.
    expect(client.getQueryData(["server", 7, "conversations", "list", ""])).toEqual({
      total: 3,
    });

    act(() => {
      result.current.restore(taken);
    });
    expect(client.getQueryData(["server", 7, "contacts", "list", ""])).toEqual({
      total: 1,
    });
    expect(client.getQueryData(["server", 7, "contacts", "list", "ada"])).toEqual({
      total: 2,
    });
  });

  it("marks every entry of the logged-in account stale, and no other account's", async () => {
    const entries = [["contacts", "list", ""], ["account-profile"], ["api-tokens"]];
    seedEntries(client, 7, entries);
    seedEntries(client, 8, entries);
    const { result } = renderHook(() => useRouteCache(), { wrapper });

    result.current.invalidateAccount();

    expect(freshEntries(client, 7, entries)).toEqual([]);
    expect(freshEntries(client, 8, entries)).toEqual(entries);
  });
});
