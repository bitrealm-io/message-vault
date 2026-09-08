/** @vitest-environment jsdom */

/**
 * One contact, read and written through one entry.
 *
 * The vault answers a change with the contact as it now stands, so the drawer
 * should show the new name without asking again — and the list pages, which
 * show the name too, should be the only thing marked stale.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useContactDetail, useUpdateContact } from "./contactDetail";
import { getContact, updateContact } from "./vaultApi";
import { keys } from "./vaultKeys";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./vaultApi")>()),
  getContact: vi.fn(),
  updateContact: vi.fn(),
}));

const read = vi.mocked(getContact);
const write = vi.mocked(updateContact);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function contact(name: string) {
  return {
    id: 7,
    name,
    last_modified: "2024-01-01T00:00:00Z",
    handles: [],
    groups: ["Family"],
    direct_conversations: 1,
    group_conversations: 0,
    message_count: 3,
  } as unknown as Awaited<ReturnType<typeof getContact>>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
});

describe("useUpdateContact", () => {
  it("puts the answered contact where the drawer reads it, without asking again", async () => {
    read.mockResolvedValue(contact("Ada"));
    write.mockResolvedValue(contact("Ada Lovelace"));

    const both = renderHook(() => ({ detail: useContactDetail("7"), update: useUpdateContact() }), {
      wrapper,
    });
    await waitFor(() => expect(both.result.current.detail.detail?.name).toBe("Ada"));
    expect(read).toHaveBeenCalledTimes(1);

    await both.result.current.update.mutateAsync({
      contactId: "7",
      body: { name: "Ada Lovelace" },
    });

    expect(write).toHaveBeenCalledWith("7", { name: "Ada Lovelace" });
    await waitFor(() => expect(both.result.current.detail.detail?.name).toBe("Ada Lovelace"));
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("marks the contact list pages stale, and not the contact it just wrote", async () => {
    write.mockResolvedValue(contact("Ada Lovelace"));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useUpdateContact(), { wrapper });
    await result.current.mutateAsync({ contactId: "7", body: { name: "Ada Lovelace" } });
    expect(invalidate.mock.calls.map((call) => call[0]?.queryKey)).toEqual([
      ["vault", 7, "contacts", "list"],
    ]);
    expect(client.getQueryData(["vault", 7, ...keys.contacts.detail("7")])).toBeDefined();
  });

  it("reports a refusal instead of writing anything", async () => {
    client.setQueryData(["vault", 7, "contacts", "detail", "7"], contact("Ada"));
    write.mockRejectedValue(new Error("handle already linked"));
    const { result } = renderHook(() => useUpdateContact(), { wrapper });
    await expect(
      result.current.mutateAsync({ contactId: "7", body: { name: "Ada Lovelace" } }),
    ).rejects.toThrow("handle already linked");
    expect(
      client.getQueryData<{ name: string }>(["vault", 7, "contacts", "detail", "7"])?.name,
    ).toBe("Ada");
  });
});
