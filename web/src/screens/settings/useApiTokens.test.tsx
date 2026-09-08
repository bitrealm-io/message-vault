/** @vitest-environment jsdom */

/**
 * What a token write leaves behind.
 *
 * The hook used to call `refetch` on its own query after each write, which
 * refreshed the list this hook holds and nothing else. It marks the list stale
 * instead, so anything showing tokens refreshes.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createApiToken, deleteApiToken, listApiTokens, renameApiToken } from "../../lib/vaultApi";
import type { ApiTokenItem } from "./apiTokensUtils";
import { useApiTokens } from "./useApiTokens";

vi.mock("../../lib/auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  listApiTokens: vi.fn(),
  createApiToken: vi.fn(),
  renameApiToken: vi.fn(),
  deleteApiToken: vi.fn(),
}));

const list = vi.mocked(listApiTokens);
const create = vi.mocked(createApiToken);
const rename = vi.mocked(renameApiToken);
const revoke = vi.mocked(deleteApiToken);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

const token: ApiTokenItem = {
  id: 1,
  label: "Laptop",
  can_import: true,
  can_export: true,
  can_delete: false,
  created_at: "1700000000",
  token_hint: "mv-api-la..op",
};

beforeEach(() => {
  vi.clearAllMocks();
  list.mockResolvedValue({ items: [token] } as unknown as Awaited<
    ReturnType<typeof listApiTokens>
  >);
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
});

describe("useApiTokens", () => {
  it("asks for the list again after a token is created", async () => {
    create.mockResolvedValue({ ...token, token: "mv-api-secret" } as unknown as Awaited<
      ReturnType<typeof createApiToken>
    >);
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(list).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.setLabel("Laptop");
    });
    act(() => {
      result.current.create();
    });

    await waitFor(() =>
      expect(create).toHaveBeenCalledWith({
        label: "Laptop",
        can_import: true,
        can_export: true,
        can_delete: false,
      }),
    );
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.reveal?.token).toBe("mv-api-secret"));
  });

  it("asks for the list again after a token is revoked, and closes the dialog either way", async () => {
    revoke.mockRejectedValue(new Error("already revoked"));
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => {
      result.current.revoke(token);
    });

    await waitFor(() => expect(result.current.actionError).toBe("already revoked"));
    expect(result.current.revokeTarget).toBeNull();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));

    act(() => {
      result.current.cancelCompose();
    });
    await waitFor(() => expect(result.current.actionError).toBe(""));
  });

  it("reports a write in flight while a rename is unanswered", async () => {
    let finish: () => void = () => {};
    rename.mockReturnValue(
      new Promise((resolve) => {
        finish = () => resolve({} as never);
      }),
    );
    const { result } = renderHook(() => useApiTokens(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => {
      result.current.openRename(token);
    });
    act(() => {
      result.current.setRenameLabel("Desktop");
    });
    act(() => {
      result.current.rename();
    });

    await waitFor(() => expect(result.current.busy).toBe(true));
    act(() => {
      finish();
    });
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(rename).toHaveBeenCalledWith(1, { label: "Desktop" });
    expect(result.current.renameTarget).toBeNull();
  });
});
