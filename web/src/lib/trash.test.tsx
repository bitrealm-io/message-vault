/** @vitest-environment jsdom */

/**
 * What each of the four trash mutations marks stale — not whether the vault
 * route was called, which proves nothing about what the app does next. Each
 * case asserts the exact set of query-key prefixes `invalidateQueries` was
 * called with, so a prefix that should stay fresh (a false positive here
 * would show up as an unwanted extra call) is checked as directly as one
 * that should go stale.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useDeleteContact,
  useDeleteConversation,
  useEmptyTrash,
  useRestoreContact,
  useRestoreConversation,
  useTrashContact,
  useTrashConversation,
} from "./trash";
import {
  deleteContact as deleteVaultContact,
  deleteConversation as deleteVaultConversation,
  emptyTrash as emptyVaultTrash,
  restoreContact as restoreVaultContact,
  restoreConversation as restoreVaultConversation,
  trashContact as trashVaultContact,
  trashConversation as trashVaultConversation,
} from "./vaultApi";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./vaultApi")>()),
  trashConversation: vi.fn(),
  restoreConversation: vi.fn(),
  deleteConversation: vi.fn(),
  trashContact: vi.fn(),
  restoreContact: vi.fn(),
  deleteContact: vi.fn(),
  emptyTrash: vi.fn(),
}));

const trashConversation = vi.mocked(trashVaultConversation);
const restoreConversation = vi.mocked(restoreVaultConversation);
const deleteConversation = vi.mocked(deleteVaultConversation);
const trashContact = vi.mocked(trashVaultContact);
const restoreContact = vi.mocked(restoreVaultContact);
const deleteContact = vi.mocked(deleteVaultContact);
const emptyTrash = vi.mocked(emptyVaultTrash);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

/** The prefixes `invalidateQueries` was asked to mark stale, account included. */
function invalidatedKeys(spy: ReturnType<typeof vi.spyOn>): unknown[] {
  return spy.mock.calls.map((call: unknown[]) => (call[0] as { queryKey?: unknown })?.queryKey);
}

beforeEach(() => {
  vi.clearAllMocks();
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
});

describe("useTrashConversation / useRestoreConversation", () => {
  it("marks the conversation list, the trash count, and open contact details stale", async () => {
    trashConversation.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useTrashConversation(), { wrapper });
    await result.current.mutateAsync(42);

    expect(trashConversation).toHaveBeenCalledWith(42, expect.anything());
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "conversations", "list"],
        ["vault", 7, "trash"],
        ["vault", 7, "contacts", "detail"],
      ]),
    );
  });

  it("leaves the conversation's own detail, its messages, and the contacts list alone", async () => {
    trashConversation.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useTrashConversation(), { wrapper });
    await result.current.mutateAsync(42);

    const keys = invalidatedKeys(invalidate);
    expect(keys).not.toContainEqual(["vault", 7, "conversations", "detail"]);
    expect(keys).not.toContainEqual(["vault", 7, "conversations", "messages"]);
    expect(keys).not.toContainEqual(["vault", 7, "contacts", "list"]);
  });

  it("restore marks the same prefixes trash does", async () => {
    restoreConversation.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useRestoreConversation(), { wrapper });
    await result.current.mutateAsync(7);

    expect(restoreConversation).toHaveBeenCalledWith(7, expect.anything());
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "conversations", "list"],
        ["vault", 7, "trash"],
        ["vault", 7, "contacts", "detail"],
      ]),
    );
  });
});

describe("useTrashContact / useRestoreContact", () => {
  it("marks the contacts list and the trashed contact's own detail stale", async () => {
    trashContact.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useTrashContact(), { wrapper });
    await result.current.mutateAsync(9);

    expect(trashContact).toHaveBeenCalledWith(9, expect.anything());
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "contacts", "list"],
        ["vault", 7, "contacts", "detail", "9"],
      ]),
    );
  });

  it("leaves every conversation query and the trash count alone", async () => {
    trashContact.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useTrashContact(), { wrapper });
    await result.current.mutateAsync(9);

    const keys = invalidatedKeys(invalidate);
    expect(keys.every((key) => (key as string[])[2] !== "conversations")).toBe(true);
    expect(keys).not.toContainEqual(["vault", 7, "trash"]);
    // Only this contact's own detail is stale, not every open drawer.
    expect(keys).not.toContainEqual(["vault", 7, "contacts", "detail"]);
  });

  it("restore addresses and invalidates the same contact it was called with", async () => {
    restoreContact.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useRestoreContact(), { wrapper });
    await result.current.mutateAsync("9");

    expect(restoreContact).toHaveBeenCalledWith("9", expect.anything());
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "contacts", "list"],
        ["vault", 7, "contacts", "detail", "9"],
      ]),
    );
  });
});

describe("useDeleteConversation", () => {
  it("marks everything about conversations stale, plus the trash count, contact details and storage", async () => {
    deleteConversation.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useDeleteConversation(), { wrapper });
    await result.current.mutateAsync(42);

    expect(deleteConversation).toHaveBeenCalledWith(42, expect.anything());
    // The whole `conversations` prefix, not only the list: the row is gone,
    // so its detail, message pages and Sources panel all describe a 404 now.
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "conversations"],
        ["vault", 7, "trash"],
        ["vault", 7, "contacts", "detail"],
        ["vault", 7, "storage"],
      ]),
    );
  });

  it("leaves the contacts list alone", async () => {
    deleteConversation.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useDeleteConversation(), { wrapper });
    await result.current.mutateAsync(42);

    const keys = invalidatedKeys(invalidate);
    expect(keys).not.toContainEqual(["vault", 7, "contacts", "list"]);
    expect(keys).not.toContainEqual(["vault", 7, "contacts"]);
  });
});

describe("useDeleteContact", () => {
  it("marks the contact's list and detail, every conversation, and the contact groups stale", async () => {
    deleteContact.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useDeleteContact(), { wrapper });
    await result.current.mutateAsync(9);

    expect(deleteContact).toHaveBeenCalledWith(9, expect.anything());
    // Conversations are marked because every one the person was in now
    // shows their handle in place of the name.
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "contacts", "list"],
        ["vault", 7, "contacts", "detail", "9"],
        ["vault", 7, "conversations"],
        ["vault", 7, "contact-groups"],
      ]),
    );
  });

  it("leaves the trash count and storage alone: no conversation or file is deleted with a contact", async () => {
    deleteContact.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useDeleteContact(), { wrapper });
    await result.current.mutateAsync(9);

    const keys = invalidatedKeys(invalidate);
    expect(keys).not.toContainEqual(["vault", 7, "trash"]);
    expect(keys).not.toContainEqual(["vault", 7, "storage"]);
  });
});

describe("useEmptyTrash", () => {
  it("marks the union of what deleting a conversation and deleting a contact mark", async () => {
    emptyTrash.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");

    const { result } = renderHook(() => useEmptyTrash(), { wrapper });
    await result.current.mutateAsync();

    expect(emptyTrash).toHaveBeenCalledTimes(1);
    expect(invalidatedKeys(invalidate)).toEqual(
      expect.arrayContaining([
        ["vault", 7, "conversations"],
        ["vault", 7, "contacts"],
        ["vault", 7, "trash"],
        ["vault", 7, "contact-groups"],
        ["vault", 7, "storage"],
      ]),
    );
  });
});
