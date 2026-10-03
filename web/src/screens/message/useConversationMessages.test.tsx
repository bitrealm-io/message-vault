/** @vitest-environment jsdom */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listConversationMessages, listMessages } from "../../lib/serverApi";
import type { Message } from "../../lib/types";
import { mockedAuth, Providers } from "../../test/providers";
import { conversationYears, useConversationMessages } from "./useConversationMessages";

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
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
      { initialProps: { id: 1 }, wrapper: Providers },
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
      { initialProps: { id: 1 }, wrapper: Providers },
    );

    rerender({ id: 2 });
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([2]));

    // Conversation 1's own entry fails; it must not touch conversation 2's.
    slow.reject(new Error("boom"));
    await slow.promise.catch(() => {});

    expect(result.current.messages.map((m) => m.id)).toEqual([2]);
    expect(result.current.loading).toBe(false);
  });

  it("opens at the newest message and reads older ones before the oldest loaded", async () => {
    // The conversation holds messages 1..120; the newest page is read newest first.
    getMessages.mockImplementation((async (_id: number, params: { before?: number }) =>
      params.before === undefined
        ? {
            items: [120, 119, 118].map(message),
            total: 120,
            limit: 50,
            offset: 0,
          }
        : {
            items: [115, 116, 117].map(message),
            total: 120,
            limit: 50,
            offset: 114,
          }) as unknown as typeof listConversationMessages);

    const { result } = renderHook(() => useConversationMessages(7), { wrapper: Providers });
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(getMessages).toHaveBeenCalledWith(
      7,
      { sort: "-date", limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    // Shown oldest first, with the newest at the bottom and nothing newer.
    expect(result.current.messages.map((m) => m.id)).toEqual([118, 119, 120]);
    expect(result.current.hasNewer).toBe(false);
    expect(result.current.hasOlder).toBe(true);
    expect(result.current.landing.to).toBe("bottom");

    act(() => result.current.loadOlder());
    await waitFor(() =>
      expect(result.current.messages.map((m) => m.id)).toEqual([115, 116, 117, 118, 119, 120]),
    );
    expect(getMessages).toHaveBeenLastCalledWith(
      7,
      { before: 118, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
  });

  it("opens at a message when asked, with the messages around it", async () => {
    getMessages.mockResolvedValue({
      items: [41, 42, 43].map(message),
      total: 90,
      limit: 50,
      offset: 40,
    });
    const { result } = renderHook(() => useConversationMessages(7, 42), { wrapper: Providers });
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(getMessages).toHaveBeenCalledWith(
      7,
      { around: 42, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    expect(result.current.highlightId).toBe(42);
    expect(result.current.landing.to).toEqual({ id: 42, align: "center" });
    expect(result.current.hasOlder).toBe(true);
    expect(result.current.hasNewer).toBe(true);
  });

  it("jumps to a year's first message, searched for in the conversation", async () => {
    getMessages.mockImplementation((async (_id: number, params: { around?: number }) => ({
      items: params.around === undefined ? [message(99)] : [message(params.around)],
      total: 99,
      limit: 50,
      offset: params.around === undefined ? 98 : 0,
    })) as unknown as typeof listConversationMessages);
    searchMessages.mockResolvedValue({ items: [message(3)], total: 40, limit: 1, offset: 0 });

    const { result } = renderHook(() => useConversationMessages(7), { wrapper: Providers });
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(() => result.current.jumpToYear(2021));
    // The first message from 2021 on: a year with no messages lands on the next one.
    expect(searchMessages).toHaveBeenCalledWith(
      { q: "in:#7 trashed:any date:>=2021", sort: "date", limit: 1 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([3]));
    expect(getMessages).toHaveBeenLastCalledWith(
      7,
      { around: 3, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    expect(result.current.landing.to).toEqual({ id: 3, align: "start" });
  });

  it("steps Find through the matches in place, newest first, without hiding the thread", async () => {
    getMessages.mockImplementation((async (_id: number, params: { around?: number }) => ({
      items: params.around === undefined ? [message(99)] : [message(params.around)],
      total: 99,
      limit: 50,
      offset: 0,
    })) as unknown as typeof listConversationMessages);
    searchMessages.mockResolvedValue({
      items: [message(60), message(20)],
      total: 2,
      limit: 50,
      offset: 0,
    });

    const { result } = renderHook(() => useConversationMessages(7), { wrapper: Providers });
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.find.openFind());
    act(() => result.current.find.setTerm("dentist"));
    // Typing jumps to the newest match, with the messages around it.
    await waitFor(() => expect(result.current.highlightId).toBe(60));
    expect(searchMessages).toHaveBeenLastCalledWith(
      { q: "in:#7 trashed:any dentist", sort: "-date", offset: 0, limit: 50 },
      expect.objectContaining({ signal: expect.anything() }),
    );
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([60]));
    expect(result.current.find.total).toBe(2);
    expect(result.current.find.position).toBe(0);

    // ▲ is the older match.
    act(() => result.current.find.prevMatch());
    await waitFor(() => expect(result.current.highlightId).toBe(20));
    expect(result.current.find.position).toBe(1);
    await waitFor(() => expect(result.current.messages.map((m) => m.id)).toEqual([20]));

    // ✕ leaves the thread where it is.
    act(() => result.current.find.close());
    expect(result.current.highlightId).toBeNull();
    expect(result.current.messages.map((m) => m.id)).toEqual([20]);

    // A phrase with a space is quoted for the language.
    act(() => result.current.find.openFind());
    act(() => result.current.find.setTerm("book club"));
    await waitFor(() =>
      expect(searchMessages).toHaveBeenLastCalledWith(
        { q: 'in:#7 trashed:any "book club"', sort: "-date", offset: 0, limit: 50 },
        expect.objectContaining({ signal: expect.anything() }),
      ),
    );
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
