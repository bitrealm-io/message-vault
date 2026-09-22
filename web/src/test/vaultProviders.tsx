/**
 * Render a component that fetches vault data.
 *
 * Anything using `useVaultQuery` needs two things from the tree: a query client
 * to cache into, and a logged-in account to name the cache entry after. This
 * supplies the first. The second comes from `useAuth`, which tests fake in the
 * usual way — see `mockedAuth` below for the shape.
 *
 * The client is built once per mounted `VaultProviders`, so no test can read a
 * cache another test filled, and retries are off so a rejected request fails
 * the test at once rather than after a delay.
 *
 * "Once per mount" rather than "once per render" is load-bearing.
 * `renderHook(...).rerender()` re-renders the wrapper, so a client built in
 * the component body was a different, empty cache on every re-render — which
 * left the tests that assert TanStack Query keeps two conversations in two
 * entries asserting it against a cache that was being thrown away underneath
 * them. `useState` holds one client for the life of the mount instead, so
 * those tests read the cache they mean to.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type RenderOptions, type RenderResult, render } from "@testing-library/react";
import { type ReactElement, type ReactNode, useState } from "react";

/**
 * A query client with retries and background refetching off.
 *
 * Entries nothing on screen reads are dropped at once, so one test's leftovers
 * never reach the next. A test about an entry written ahead of the screen that
 * reads it (a seed from a mutation's answer) passes `keepUnread`, which holds
 * such entries the way the app's client does.
 */
export function testQueryClient({
  keepUnread = false,
}: {
  keepUnread?: boolean;
} = {}): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        staleTime: 0,
        gcTime: keepUnread ? Number.POSITIVE_INFINITY : 0,
      },
      mutations: { retry: false },
    },
  });
}

/** Wrap children in a query client that lives as long as this mount. */
export function VaultProviders({
  children,
  keepUnread = false,
}: {
  children: ReactNode;
  /** Hold entries nothing on screen reads; see `testQueryClient`. */
  keepUnread?: boolean;
}) {
  const [client] = useState(() => testQueryClient({ keepUnread }));
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

/** `render`, with the query client a vault query needs. */
export function renderWithVault(ui: ReactElement, options?: RenderOptions): RenderResult {
  return render(ui, { wrapper: VaultProviders, ...options });
}

/**
 * What `vi.mock("<path>/auth")` should return so a vault query has an account
 * to name its cache entry after.
 *
 * Any id works; what matters is that it is stable within a test, since a
 * changing account id is a different cache entry by design.
 */
export const mockedAuth = {
  accountId: 7,
  token: "test-token",
  isAuthenticated: true,
};
