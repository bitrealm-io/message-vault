/** @vitest-environment jsdom */

/**
 * What the owner's deletions leave marked stale.
 *
 * Deleting an account or its messages changes what the whole database holds
 * (the Dashboard) and, for the Demo Account, the Demo Account card. Both kept
 * their old figures when a deletion marked only the account list stale.
 */

import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { deleteAccountById, deleteAccountMessages } from "../../lib/serverApi";
import { testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { useDeleteAccount, useDeleteAccountMessages } from "./useOwnerAccounts";

vi.mock("../../lib/auth", () => ({ useAuth: () => ({ accountId: 1 }) }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  deleteAccountById: vi.fn(),
  deleteAccountMessages: vi.fn(),
}));

const shown = [keys.ownerAccounts.all, keys.serverStorage.all, keys.demoAccount.all];

let client = testQueryClient();

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = testQueryClient();
  seedEntries(client, 1, shown);
});

describe("the owner's deletions", () => {
  it("deleting an account marks the account list, the Dashboard and the Demo Account stale", async () => {
    vi.mocked(deleteAccountById).mockResolvedValue(undefined);
    const { result } = renderHook(() => useDeleteAccount(), { wrapper });

    await result.current.mutateAsync(2);

    expect(deleteAccountById).toHaveBeenCalledWith(2);
    expect(freshEntries(client, 1, shown)).toEqual([]);
  });

  it("deleting an account's messages marks the same entries stale", async () => {
    vi.mocked(deleteAccountMessages).mockResolvedValue({ conversations: 3, attachments: 0 });
    const { result } = renderHook(() => useDeleteAccountMessages(), { wrapper });

    await result.current.mutateAsync(2);

    expect(deleteAccountMessages).toHaveBeenCalledWith(2);
    expect(freshEntries(client, 1, shown)).toEqual([]);
  });
});
