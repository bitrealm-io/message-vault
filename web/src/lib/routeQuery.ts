/**
 * The one way the web app fetches and remembers server data.
 *
 * TanStack Query does the remembering, the duplicate-request suppression, the
 * loading and error state, and the "this is stale now, refetch whoever is
 * showing it" broadcast. Nothing here reimplements any of that; the only thing
 * this module adds is the rule that every cache entry is named with the
 * logged-in account.
 *
 * That rule is the point. Before it, four modules kept the account's data in
 * module-level variables and `auth.tsx` cleared them by hand from two separate
 * lists — both of which named the same four and omitted the fifth, so a second
 * account could be shown the first account's Saved Searches. Naming the account
 * in the key makes that impossible rather than merely unlikely: a second
 * account asks for an entry that has never been written, finds nothing, and
 * fetches.
 *
 * See `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */

import {
  type InfiniteData,
  MutationCache,
  QueryCache,
  QueryClient,
  type UseInfiniteQueryOptions,
  type UseInfiniteQueryResult,
  type UseQueryOptions,
  type UseQueryResult,
  useInfiniteQuery,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useEffect, useMemo } from "react";
import { ApiError } from "./api";
import { useAuth } from "./auth";
import { PAGE_SIZE_FILL, PAGE_SIZE_FIRST } from "./listPaging";
import {
  type AccountScope,
  ANONYMOUS_ACCOUNT,
  type RouteQueryKey,
  routeQueryKey,
} from "./routeQueryKey";

/**
 * Whether a failure says the session token is no longer any good.
 *
 * The server answers `401 Unauthorized` both for a token it no longer accepts
 * (`authentication-required`) and for a mistyped current password
 * (`invalid-credentials`). Only the first ends the session: a wrong password
 * typed into Settings must not log the person out.
 */
function endsSession(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.type !== "invalid-credentials";
}

/**
 * Build the query client.
 *
 * Exported as a factory rather than a singleton so each test gets a client of
 * its own and cannot inherit another test's cache.
 *
 * `onUnauthorized` runs when any query or mutation fails because the server no
 * longer accepts the session token. Without it every screen showed its own
 * error and the person was never sent back to the login screen.
 */
export function createQueryClient({
  onUnauthorized = () => {},
}: {
  onUnauthorized?: () => void;
} = {}): QueryClient {
  const onError = (error: unknown) => {
    if (endsSession(error)) onUnauthorized();
  };
  return new QueryClient({
    queryCache: new QueryCache({ onError }),
    mutationCache: new MutationCache({ onError }),
    defaultOptions: {
      queries: {
        // The server is usually on the same host or a local network, so a
        // refetch is cheap. Half a minute is long enough that moving between
        // screens does not refetch, and short enough that a stale list
        // corrects itself without anyone reloading.
        staleTime: 30_000,
        // One retry, except for an ended session: asking again with the same
        // token gets the same answer, and only delays the login screen.
        retry: (failureCount, error) => !endsSession(error) && failureCount < 1,
        refetchOnWindowFocus: true,
      },
    },
  });
}

/**
 * Logged-in account id, or `"anonymous"` before login.
 *
 * Queries that run on the login screens have no account yet; giving them a
 * name of their own keeps their entries from ever being read by a logged-in
 * account.
 */
function useAccountScope(): AccountScope {
  const { accountId } = useAuth();
  return accountId ?? ANONYMOUS_ACCOUNT;
}

/**
 * `useQuery`, with the logged-in account added to the front of the key.
 *
 * Every option TanStack Query accepts is passed straight through. This adds no
 * caching, no fetching, and no state of its own — only the account prefix, so
 * that no call site has to remember it.
 */
export function useRouteQuery<TData>(
  key: RouteQueryKey,
  queryFn: (signal: AbortSignal) => Promise<TData>,
  options?: Omit<UseQueryOptions<TData, Error, TData>, "queryKey" | "queryFn">,
): UseQueryResult<TData, Error> {
  const account = useAccountScope();
  return useQuery<TData, Error, TData>({
    queryKey: routeQueryKey(account, key),
    queryFn: ({ signal }) => queryFn(signal),
    ...options,
  });
}

/**
 * `useInfiniteQuery`, with the logged-in account added to the front of the key.
 *
 * Like `useRouteQuery`, every option is passed straight through; this adds
 * only the account prefix. For a list read outward in both directions from a
 * place in it, which `useRoutePagedList`'s forward-only offsets cannot do.
 */
export function useRouteInfiniteQuery<TPage, TPageParam>(
  key: RouteQueryKey,
  options: Omit<
    UseInfiniteQueryOptions<TPage, Error, InfiniteData<TPage, TPageParam>, unknown[], TPageParam>,
    "queryKey"
  >,
): UseInfiniteQueryResult<InfiniteData<TPage, TPageParam>, Error> {
  const account = useAccountScope();
  return useInfiniteQuery<TPage, Error, InfiniteData<TPage, TPageParam>, unknown[], TPageParam>({
    queryKey: routeQueryKey(account, key),
    ...options,
  });
}

/** One page of an offset-paged list, with the total the server reported. */
export type OffsetPage<T> = {
  items: T[];
  total: number;
};

/**
 * Stands in for `pages` before the first page has arrived.
 *
 * `query.data?.pages ?? []` looks harmless, but that `[]` is a fresh array on
 * every call — pending or not — so a consumer that memoizes off `pages` never
 * sees two equal renders while the query is still loading. One shared
 * constant, cast per call site, keeps that fallback a single reference.
 */
const NO_PAGES: readonly unknown[] = [];

/** How a screen loads one page. */
export type PagedFetchPage<T> = (args: {
  limit: number;
  offset: number;
  signal: AbortSignal;
}) => Promise<OffsetPage<T>>;

/** What a long list needs to render itself while it fills. */
export type PagedListResult<T> = {
  items: T[];
  total: number;
  /** No page has arrived yet: the list has nothing to show. */
  loading: boolean;
  /** The first page is being fetched again behind rows already on screen. */
  refreshing: boolean;
  /** A later page is loading because the person scrolled near the end. */
  filling: boolean;
  error: Error | null;
  hasMore: boolean;
  loadMore: () => void;
};

/**
 * An offset-paged server list, account-scoped like every other cache entry.
 *
 * The server pages by `limit` and `offset` and reports a `total`, so the next
 * page starts where the pages loaded so far end and there is no next page once
 * they cover the total.
 *
 * This returns the shape a long list renders from rather than TanStack Query's
 * own result, so the two screens that use it do not each repeat the same
 * mapping from `isPending` / `isFetchingNextPage` to "loading" and "filling".
 *
 * Offsets move when a row is added or removed between two page fetches, by an
 * import, a move to the Trash, or another tab. The next page can then repeat a
 * row already on screen or start past one never shown. A repeated row is shown
 * once, by its id. A page whose `total` differs from the first page's means
 * the offsets moved, so the list is fetched again from offset 0. A row added
 * and another removed between two fetches leaves the total the same, and a
 * row skipped that way stays missing until the list is next fetched.
 */
export function useRoutePagedList<T extends { id: string | number }>(
  key: RouteQueryKey,
  fetchPage: PagedFetchPage<T>,
  opts?: {
    firstPageSize?: number;
    fillPageSize?: number;
    /**
     * The largest `offset` the route accepts. The list ends where the next
     * page would start past it, even when `total` names more rows.
     */
    maxOffset?: number;
  },
): PagedListResult<T> {
  const account = useAccountScope();
  const firstPageSize = opts?.firstPageSize ?? PAGE_SIZE_FIRST;
  const fillPageSize = opts?.fillPageSize ?? PAGE_SIZE_FILL;
  const maxOffset = opts?.maxOffset ?? Number.POSITIVE_INFINITY;

  const query = useInfiniteQuery<
    OffsetPage<T>,
    Error,
    InfiniteData<OffsetPage<T>>,
    unknown[],
    number
  >({
    queryKey: routeQueryKey(account, key),
    initialPageParam: 0,
    queryFn: ({ pageParam, signal }) =>
      fetchPage({
        limit: pageParam === 0 ? firstPageSize : fillPageSize,
        offset: pageParam,
        signal,
      }),
    getNextPageParam: (_lastPage, pages) => {
      const loaded = pages.reduce((sum, page) => sum + page.items.length, 0);
      const total = pages[pages.length - 1]?.total ?? 0;
      return loaded < total && loaded <= maxOffset ? loaded : undefined;
    },
  });

  const pages = query.data?.pages ?? (NO_PAGES as OffsetPage<T>[]);
  // A new array every render defeats every memo downstream (the tag menu and
  // its effect included), so this is the one place that must not recompute
  // unless the query actually produced new pages.
  const items = useMemo(() => {
    const seen = new Set<string | number>();
    const rows: T[] = [];
    for (const page of pages) {
      for (const row of page.items) {
        if (seen.has(row.id)) continue;
        seen.add(row.id);
        rows.push(row);
      }
    }
    return rows;
  }, [pages]);

  const totalMoved = pages.some((page) => page.total !== pages[0]?.total);
  const { isFetching, refetch } = query;
  useEffect(() => {
    // A refetch starts at the first page's offset, 0, and works out every
    // later offset again from the pages it fetches.
    if (totalMoved && !isFetching) void refetch();
  }, [totalMoved, isFetching, refetch]);

  return {
    items,
    total: pages[pages.length - 1]?.total ?? 0,
    loading: query.isPending,
    // A refetch of what is already on screen, as opposed to a first load or a
    // page being appended.
    refreshing: query.isFetching && !query.isFetchingNextPage && !query.isPending,
    filling: query.isFetchingNextPage,
    error: query.error,
    hasMore: query.hasNextPage,
    loadMore: () => {
      if (query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage();
    },
  };
}

/** Entries as `snapshot` took them. The key is complete, account included. */
export type RouteCacheEntries = readonly [readonly unknown[], unknown][];

/**
 * The cache operations a write needs, with the account already in front of
 * every key.
 *
 * A mutation draws its change before the server answers, puts the old value
 * back when the server refuses, and marks the account stale once it settles. Each of
 * those is one call on the query client — this is that client with the account
 * rule applied, and nothing else. It is not a cache.
 */
export type RouteCache = {
  /** What the cache holds under one key, without fetching. */
  read: <T>(key: RouteQueryKey) => T | undefined;
  /** Ask the server now and store the answer under the key. */
  fetch: <T>(key: RouteQueryKey, queryFn: (signal: AbortSignal) => Promise<T>) => Promise<T>;
  /** Write one entry, for a mutation that answered with the whole value. */
  set: <T>(key: RouteQueryKey, value: T) => void;
  /** Stop fetches under a prefix, so none lands on top of an optimistic write. */
  cancel: (prefix: RouteQueryKey) => Promise<void>;
  /** Every entry under a prefix, as it stands, to put back on failure. */
  snapshot: (prefix: RouteQueryKey) => RouteCacheEntries;
  /** Rewrite every entry under a prefix. */
  patch: <T>(prefix: RouteQueryKey, update: (entry: T | undefined) => T | undefined) => void;
  /** Put snapshotted entries back where they came from. */
  restore: (entries: RouteCacheEntries) => void;
  /**
   * Mark every entry of the logged-in account stale, so whatever is on screen
   * refetches. Every write calls this once it settles.
   *
   * The whole account, not a list of the entries one write changes: six
   * writes once left out entries they changed, and the screens showing those
   * kept the old state. TanStack Query refetches only the entries on screen,
   * so the cost is a few small requests. See
   * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
   *
   * The entries are marked before this returns. The refetches it starts are
   * not waited for, so a write is finished when the server has answered it,
   * not when every screen has fetched again.
   */
  invalidateAccount: () => void;
};

export function useRouteCache(): RouteCache {
  const client = useQueryClient();
  const account = useAccountScope();
  return useMemo(() => {
    const at = (key: RouteQueryKey) => routeQueryKey(account, key);
    return {
      read: <T>(key: RouteQueryKey) => client.getQueryData<T>(at(key)),
      fetch: <T>(key: RouteQueryKey, queryFn: (signal: AbortSignal) => Promise<T>) =>
        client.fetchQuery<T>({
          queryKey: at(key),
          queryFn: ({ signal }) => queryFn(signal),
          staleTime: 0,
        }),
      set: <T>(key: RouteQueryKey, value: T) => {
        client.setQueryData(at(key), value);
      },
      cancel: (prefix: RouteQueryKey) => client.cancelQueries({ queryKey: at(prefix) }),
      snapshot: (prefix: RouteQueryKey) => client.getQueriesData({ queryKey: at(prefix) }),
      patch: <T>(prefix: RouteQueryKey, update: (entry: T | undefined) => T | undefined) => {
        client.setQueriesData<T>({ queryKey: at(prefix) }, update);
      },
      restore: (entries: RouteCacheEntries) => {
        for (const [key, data] of entries) client.setQueryData(key, data);
      },
      invalidateAccount: () => {
        void client.invalidateQueries({ queryKey: at([]) });
      },
    };
  }, [client, account]);
}

export type { RouteQueryKey };
