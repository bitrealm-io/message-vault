/** @vitest-environment jsdom */

/**
 * What each trash mutation sends, and what it leaves marked stale.
 *
 * Every write marks the whole account's cache stale. The entries each case
 * names are the ones a screen showed out of date when a write left them out,
 * so a regression names the screen it breaks.
 */

import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testQueryClient } from "../test/providers";
import { freshEntries, seedEntries } from "../test/staleEntries";
import { keys } from "./queryKeys";
import {
  deleteContact as deleteContactRoute,
  deleteConversation as deleteConversationRoute,
  emptyTrash as emptyTrashRoute,
  restoreContact as restoreContactRoute,
  restoreConversation as restoreConversationRoute,
  trashContact as trashContactRoute,
  trashConversation as trashConversationRoute,
} from "./serverApi";
import {
  useDeleteContact,
  useDeleteConversation,
  useEmptyTrash,
  useRestoreContact,
  useRestoreConversation,
  useTrashContact,
  useTrashConversation,
} from "./trash";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./serverApi")>()),
  trashConversation: vi.fn(),
  restoreConversation: vi.fn(),
  deleteConversation: vi.fn(),
  trashContact: vi.fn(),
  restoreContact: vi.fn(),
  deleteContact: vi.fn(),
  emptyTrash: vi.fn(),
}));

const trashConversation = vi.mocked(trashConversationRoute);
const restoreConversation = vi.mocked(restoreConversationRoute);
const deleteConversation = vi.mocked(deleteConversationRoute);
const trashContact = vi.mocked(trashContactRoute);
const restoreContact = vi.mocked(restoreContactRoute);
const deleteContact = vi.mocked(deleteContactRoute);
const emptyTrash = vi.mocked(emptyTrashRoute);

let client = testQueryClient();

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = testQueryClient();
});

/** Run `write` with every entry in `shown` seeded, and answer which stayed fresh. */
async function freshAfter(
  shown: readonly (readonly unknown[])[],
  write: () => Promise<unknown>,
): Promise<unknown[]> {
  seedEntries(client, 7, shown);
  await write();
  return freshEntries(client, 7, shown);
}

/** The conversation list, a find, the Trash count, and an open contact's counts. */
const conversationTrashState = [
  keys.conversations.lists,
  keys.conversations.find(42, "date:2020", "-date", 0, 50),
  keys.trash.all,
  keys.contacts.details,
];

/** The account's message count and storage, and each identity's message count. */
const accountCounts = [keys.accountProfile.all, keys.accountProfile.identities];

describe("useTrashConversation / useRestoreConversation", () => {
  it("trash marks every list and count that shows trash state stale", async () => {
    trashConversation.mockResolvedValue(undefined);
    const { result } = renderHook(() => useTrashConversation(), { wrapper });

    const fresh = await freshAfter(conversationTrashState, () => result.current.mutateAsync(42));

    expect(trashConversation).toHaveBeenCalledWith(42, expect.anything());
    expect(fresh).toEqual([]);
  });

  it("restore marks the same entries stale", async () => {
    restoreConversation.mockResolvedValue(undefined);
    const { result } = renderHook(() => useRestoreConversation(), { wrapper });

    const fresh = await freshAfter(conversationTrashState, () => result.current.mutateAsync(7));

    expect(restoreConversation).toHaveBeenCalledWith(7, expect.anything());
    expect(fresh).toEqual([]);
  });
});

describe("useTrashContact / useRestoreContact", () => {
  const shown = [keys.contacts.lists, keys.contacts.detail(9)];

  it("trash marks the contacts list and the contact's own detail stale", async () => {
    trashContact.mockResolvedValue(undefined);
    const { result } = renderHook(() => useTrashContact(), { wrapper });

    const fresh = await freshAfter(shown, () => result.current.mutateAsync(9));

    expect(trashContact).toHaveBeenCalledWith(9, expect.anything());
    expect(fresh).toEqual([]);
  });

  it("restore addresses the contact it was called with", async () => {
    restoreContact.mockResolvedValue(undefined);
    const { result } = renderHook(() => useRestoreContact(), { wrapper });

    const fresh = await freshAfter(shown, () => result.current.mutateAsync("9"));

    expect(restoreContact).toHaveBeenCalledWith("9", expect.anything());
    expect(fresh).toEqual([]);
  });
});

describe("useDeleteConversation", () => {
  it("marks the conversation, the Trash, storage and the account's counts stale", async () => {
    deleteConversation.mockResolvedValue(undefined);
    const { result } = renderHook(() => useDeleteConversation(), { wrapper });

    // The account's message count and storage, and each identity's count,
    // kept their old figures when only the conversation entries were marked.
    const fresh = await freshAfter(
      [
        keys.conversations.all,
        keys.trash.all,
        keys.contacts.details,
        keys.storage.all,
        ...accountCounts,
      ],
      () => result.current.mutateAsync(42),
    );

    expect(deleteConversation).toHaveBeenCalledWith(42, expect.anything());
    expect(fresh).toEqual([]);
  });
});

describe("useDeleteContact", () => {
  it("marks the contact, every conversation that named it, and the Contact Groups stale", async () => {
    deleteContact.mockResolvedValue(undefined);
    const { result } = renderHook(() => useDeleteContact(), { wrapper });

    const fresh = await freshAfter(
      [
        keys.contacts.lists,
        keys.contacts.detail(9),
        keys.conversations.all,
        keys.contactGroups.all,
      ],
      () => result.current.mutateAsync(9),
    );

    expect(deleteContact).toHaveBeenCalledWith(9, expect.anything());
    expect(fresh).toEqual([]);
  });
});

describe("useEmptyTrash", () => {
  it("marks what deleting a conversation and a contact mark, and the account's counts, stale", async () => {
    emptyTrash.mockResolvedValue(undefined);
    const { result } = renderHook(() => useEmptyTrash(), { wrapper });

    const fresh = await freshAfter(
      [
        keys.conversations.all,
        keys.contacts.all,
        keys.trash.all,
        keys.contactGroups.all,
        keys.storage.all,
        ...accountCounts,
      ],
      () => result.current.mutateAsync(),
    );

    expect(emptyTrash).toHaveBeenCalledTimes(1);
    expect(fresh).toEqual([]);
  });
});
