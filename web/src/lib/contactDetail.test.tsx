/** @vitest-environment jsdom */

/**
 * One contact, read and written through one entry.
 *
 * The server answers a change with the contact as it now stands, so the drawer
 * shows the new name before anything is fetched again. Every other screen that
 * shows the name is marked stale with the rest of the account's cache.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { freshEntries, seedEntries } from "../test/staleEntries";
import { useContactDetail, useUpdateContact } from "./contactDetail";
import { keys } from "./queryKeys";
import { getContact, updateContact } from "./serverApi";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./serverApi")>()),
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
    addresses: [],
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
  it("puts the answered contact where the drawer reads it, without waiting for a read", async () => {
    read.mockResolvedValueOnce(contact("Ada"));
    // The read every write starts never answers, so only the write's own
    // answer can show the new name.
    read.mockReturnValue(new Promise(() => {}));
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
  });

  it("marks the contact list and the conversations stale, which show the contact's name", async () => {
    // Conversation rows, an open conversation and its message pages name a
    // participant by the contact's name, so a rename left the old one there.
    const shown = [keys.contacts.lists, keys.conversations.all];
    seedEntries(client, 7, shown);
    write.mockResolvedValue(contact("Mum"));
    const { result } = renderHook(() => useUpdateContact(), { wrapper });
    await result.current.mutateAsync({ contactId: "7", body: { name: "Mum" } });
    expect(freshEntries(client, 7, shown)).toEqual([]);
  });

  it("reports a refusal instead of writing anything", async () => {
    client.setQueryData(["server", 7, "contacts", "detail", "7"], contact("Ada"));
    write.mockRejectedValue(new Error("handle already linked"));
    const { result } = renderHook(() => useUpdateContact(), { wrapper });
    await expect(
      result.current.mutateAsync({ contactId: "7", body: { name: "Ada Lovelace" } }),
    ).rejects.toThrow("handle already linked");
    expect(
      client.getQueryData<{ name: string }>(["server", 7, "contacts", "detail", "7"])?.name,
    ).toBe("Ada");
  });
});
