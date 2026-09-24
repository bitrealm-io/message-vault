/** @vitest-environment jsdom */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Message } from "../../lib/types";
import { listConversationMessages, listMessages } from "../../lib/vaultApi";
import { mockedAuth, VaultProviders } from "../../test/vaultProviders";
import {
  buildFooterLabel,
  conversationYears,
  useConversationMessages,
} from "./useConversationMessages";

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  listConversationMessages: vi.fn(),
  listMessages: vi.fn(),
}));

const getMessages = vi.mocked(listConversationMessages);
const searchMessages = vi.mocked(listMessages);

function message(id: number): Message {
  return {
    id,
    source: "test",
    service: "sms",
    guid: null,
    timestamp: "2024-01-01T00:00:00Z",
    is_from_me: false,
    sender: "someone",
    subject: null,
    text: `from-${id}`,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: id,
    conversation: {
      id: 1,
      chat_identifier: "c",
      conversation_type: "direct",
      group_title: null,
      participants: [],
    },
    attachments: [],
    tapbacks: [],
  };
}

/** A promise plus the handles that settle it, so a test can control landing order. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // Nothing else awaits this promise once the request is superseded.
  promise.catch(() => {});
  return { promise, resolve, reject };
}

type MessagePage = { items: Message[]; total: number; limit: number; offset: number };
function page(items: Message[]): MessagePage {
  return { items, total: items.length, limit: 50, offset: 0 };
}

/** Route the mocked calls: conversation 1 hangs on `slow`, conversation 2 answers at once. */
function routeGets(slow: Promise<MessagePage>) {
  getMessages.mockImplementation(((id: number) =>
    id === 1
      ? slow
      : Promise.resolve(page([message(2)]))) as unknown as typeof listConversationMessages);
}

describe("useConversationMessages", () => {
  beforeEach(() => {
    getMessages.mockReset();
    searchMessages.mockReset();
  });

  it("ignores a slow response from the conversation the user navigated away from", async () => {
    const slow = deferred<MessagePage>();
    routeGets(slow.promise);

    const { result, rerender } = renderHook(
      ({ id }: { id: number }) => useConversationMessages(id),
      { initialProps: { id: 1 }, wrapper: VaultProviders },
    );

    rerender({ id: 2 });
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([2]));

    slow.resolve(page([message(1)]));
    await slow.promise;

    // Conversation 1's own cache entry now holds its answer, but the hook is
    // reading conversation 2's entry, so its messages are untouched.
    expect(result.current.messages.map((m) => m.id)).toEqual([2]);
    expect(result.current.loading).toBe(false);
  });

  it("keeps the current page when a request rejects", async () => {
    const slow = deferred<MessagePage>();
    routeGets(slow.promise);

    const { result, rerender } = renderHook(
      ({ id }: { id: number }) => useConversationMessages(id),
      { initialProps: { id: 1 }, wrapper: VaultProviders },
    );

    rerender({ id: 2 });
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([2]));

    // Conversation 1's own entry fails; it must not touch conversation 2's.
    slow.reject(new Error("boom"));
    await slow.promise.catch(() => {});

    expect(result.current.messages.map((m) => m.id)).toEqual([2]);
    expect(result.current.loading).toBe(false);
  });

  it("asks for a year as a search in the conversation, one page at a time", async () => {
    getMessages.mockResolvedValueOnce(page([message(9)]));
    searchMessages
      .mockResolvedValueOnce({ items: [message(1), message(2)], total: 3, limit: 50, offset: 0 })
      .mockResolvedValueOnce({ items: [message(3)], total: 3, limit: 50, offset: 50 });

    const { result } = renderHook(({ id }: { id: number }) => useConversationMessages(id), {
      initialProps: { id: 7 },
      wrapper: VaultProviders,
    });
    await waitFor(() => expect(result.current.loading).toBe(false));

    // Browsing all years carries no year at all.
    expect(getMessages).toHaveBeenNthCalledWith(
      1,
      7,
      { offset: 0, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );

    act(() => result.current.selectYear(2020));
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([1, 2]));

    // Opening a conversation takes no filter, so a year is `date:` in a
    // search scoped to the conversation, at the ordinary page size. A year is
    // not loaded in full (#323), and a 50,000-message year no longer hits the
    // offset ceiling mid-walk (#326).
    expect(searchMessages).toHaveBeenNthCalledWith(
      1,
      { q: "in:#7 trashed:any date:2020", offset: 0, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    expect(result.current.total).toBe(3);
    expect(result.current.finding).toBe(false);

    act(() => result.current.fetchConversationPage(50));
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([3]));
    expect(searchMessages).toHaveBeenNthCalledWith(
      2,
      { q: "in:#7 trashed:any date:2020", offset: 50, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    expect(getMessages).toHaveBeenCalledTimes(1);
    expect(searchMessages).toHaveBeenCalledTimes(2);
  });

  it("runs the find box on the vault, scoped to the conversation and the chosen year", async () => {
    getMessages.mockResolvedValue(page([message(9)]));
    searchMessages.mockResolvedValue({
      items: [message(4), message(5)],
      total: 2,
      limit: 50,
      offset: 0,
    });

    const { result } = renderHook(({ id }: { id: number }) => useConversationMessages(id), {
      initialProps: { id: 7 },
      wrapper: VaultProviders,
    });
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.setFindTerm("dentist"));
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([4, 5]));
    expect(result.current.finding).toBe(true);
    expect(result.current.total).toBe(2);
    // `in:#id` plus the term as free text: the same language every list
    // speaks. `trashed:any` keeps a conversation opened from the Trash
    // screen searchable.
    expect(searchMessages).toHaveBeenLastCalledWith(
      { q: "in:#7 trashed:any dentist", offset: 0, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );

    act(() => result.current.selectYear(2021));
    await waitFor(() =>
      expect(searchMessages).toHaveBeenLastCalledWith(
        { q: "in:#7 trashed:any date:2021 dentist", offset: 0, limit: 50 },
        expect.objectContaining({ signal: expect.anything() }),
      ),
    );

    // A phrase with a space is quoted for the language.
    act(() => result.current.setFindTerm("book club"));
    await waitFor(() =>
      expect(searchMessages).toHaveBeenLastCalledWith(
        { q: 'in:#7 trashed:any date:2021 "book club"', offset: 0, limit: 50 },
        expect.objectContaining({ signal: expect.anything() }),
      ),
    );

    // Clearing the box returns to the thread.
    act(() => result.current.setFindTerm(""));
    await waitFor(() => expect(result.current.finding).toBe(false));
  });

  it("resets offset, activeYear, findTerm and activeMatch when the conversation changes", async () => {
    getMessages.mockResolvedValue(page([message(1)]));

    const { result, rerender } = renderHook(
      ({ id }: { id: number }) => useConversationMessages(id),
      { initialProps: { id: 1 }, wrapper: VaultProviders },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));

    // Drive every reset-target away from its default before switching. A new
    // find term starts at page one, so the page turn comes after it.
    act(() => result.current.selectYear(2020));
    act(() => result.current.setFindTerm("hello"));
    act(() => result.current.fetchConversationPage(50));
    act(() => result.current.setActiveMatch(3));

    expect(result.current.activeYear).toBe(2020);
    expect(result.current.offset).toBe(50);
    expect(result.current.findTerm).toBe("hello");
    expect(result.current.activeMatch).toBe(3);

    rerender({ id: 2 });

    expect(result.current.activeYear).toBeNull();
    expect(result.current.offset).toBe(0);
    expect(result.current.findTerm).toBe("");
    expect(result.current.activeMatch).toBe(0);
  });
});

describe("conversationYears", () => {
  it("covers both endpoint years", () => {
    expect(conversationYears("2020-05-01T00:00:00Z", "2022-02-01T00:00:00Z", "UTC")).toEqual([
      2020, 2021, 2022,
    ]);
  });

  it("reads the years in the account's zone", () => {
    // The last message is 04:59 UTC on New Year's Day: still 2024 in New
    // York, so no 2025 chip is offered for a year that holds nothing.
    expect(conversationYears("2024-06-01T00:00:00Z", "2025-01-01T04:59:00Z", "UTC")).toEqual([
      2024, 2025,
    ]);
    expect(
      conversationYears("2024-06-01T00:00:00Z", "2025-01-01T04:59:00Z", "America/New_York"),
    ).toEqual([2024]);
  });

  it("returns nothing without both endpoints", () => {
    expect(conversationYears(null, "2022-02-01T00:00:00Z", "UTC")).toEqual([]);
  });
});

describe("buildFooterLabel", () => {
  it("shows the page window for a year filter, since a year pages like everything else", () => {
    expect(buildFooterLabel(2021, 120, 0)).toBe("2021: 1–50 of 120");
    expect(buildFooterLabel(2021, 120, 100)).toBe("2021: 101–120 of 120");
  });

  it("shows the page window when browsing all years", () => {
    expect(buildFooterLabel(null, 120, 50)).toBe("Messages 51–100 of 120");
  });

  it("names the rows as matches while finding", () => {
    expect(buildFooterLabel(null, 7, 0, true)).toBe("Matches 1–7 of 7");
    expect(buildFooterLabel(2021, 0, 0, true)).toBe("Matches 0 of 0");
  });
});
