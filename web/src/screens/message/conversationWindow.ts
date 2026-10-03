import { listConversationMessages } from "../../lib/serverApi";
import type { Message } from "../../lib/types";

/** Messages read per request, in either direction. */
export const PAGE_SIZE = 50;

/**
 * Where the conversation panel opens or jumps to: the newest message, or a
 * message by id (a year's first message, a Find match, a search result).
 */
export type WindowStart = { kind: "newest" } | { kind: "around"; id: number };

/** What one request reads: the start, or the page just before or after a loaded message. */
export type WindowPageParam =
  | WindowStart
  | { kind: "before"; id: number }
  | { kind: "after"; id: number };

/**
 * One page of the conversation, oldest first, with where it sits: `offset`
 * messages come before it and `total` are in the conversation.
 */
export type WindowPage = { items: Message[]; total: number; offset: number };

/** The cache key part naming a start. */
export function startKey(start: WindowStart): string {
  return start.kind === "newest" ? "newest" : `around:${start.id}`;
}

/**
 * Read one page. The newest page is read newest first, the one way to ask
 * for the end of the conversation without knowing its length, and turned
 * round so every page the panel holds runs oldest first.
 */
export async function fetchWindowPage(
  conversationId: number,
  param: WindowPageParam,
  signal: AbortSignal,
): Promise<WindowPage> {
  if (param.kind === "newest") {
    const page = await listConversationMessages(
      conversationId,
      { sort: "-date", limit: PAGE_SIZE },
      { signal },
    );
    return {
      items: [...page.items].reverse(),
      total: page.total,
      offset: Math.max(0, page.total - page.offset - page.items.length),
    };
  }
  const page = await listConversationMessages(
    conversationId,
    { [param.kind]: param.id, limit: PAGE_SIZE },
    { signal },
  );
  return { items: page.items, total: page.total, offset: page.offset };
}

/** The page before the oldest one loaded, or `undefined` when it starts the conversation. */
export function olderPageParam(oldest: WindowPage): WindowPageParam | undefined {
  const first = oldest.items[0];
  return first && oldest.offset > 0 ? { kind: "before", id: first.id } : undefined;
}

/** The page after the newest one loaded, or `undefined` when it ends the conversation. */
export function newerPageParam(newest: WindowPage): WindowPageParam | undefined {
  const last = newest.items[newest.items.length - 1];
  return last && newest.offset + newest.items.length < newest.total
    ? { kind: "after", id: last.id }
    : undefined;
}
