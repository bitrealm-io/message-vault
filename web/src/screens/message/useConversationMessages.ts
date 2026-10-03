import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { keys } from "../../lib/queryKeys";
import { useRouteCache, useRouteInfiniteQuery, useRouteQuery } from "../../lib/routeQuery";
import { quote } from "../../lib/searchQuery";
import { listMessages } from "../../lib/serverApi";
import { yearIn } from "../../lib/timeZone";
import type { Message } from "../../lib/types";
import {
  fetchWindowPage,
  newerPageParam,
  olderPageParam,
  PAGE_SIZE,
  startKey,
  type WindowPage,
  type WindowPageParam,
  type WindowStart,
} from "./conversationWindow";

export { PAGE_SIZE } from "./conversationWindow";

/**
 * Calendar years covered by a conversation's first and last message instants,
 * read in the account's `zone`: the same rule `date:2024` uses, so the first
 * and last years named have messages in them. A year between them is named
 * whether or not it has messages; jumping to it lands on the next message.
 */
export function conversationYears(
  startIso: string | null | undefined,
  endIso: string | null | undefined,
  zone: string,
): number[] {
  if (!startIso || !endIso) return [];
  const startYear = yearIn(startIso, zone);
  const endYear = yearIn(endIso, zone);
  if (!Number.isFinite(startYear) || !Number.isFinite(endYear) || endYear < startYear) {
    return [];
  }
  const years: number[] = [];
  for (let y = startYear; y <= endYear; y++) years.push(y);
  return years;
}

/**
 * The search-language query for messages of one conversation: the
 * conversation by id, whether or not it is in the trash, the `date` span when
 * one is given, and the typed find term, when there is one, as free text.
 * Runs on `GET /v1/messages`, so it reaches every message in the
 * conversation, not the pages in hand. Opening a conversation takes no
 * filter; a search inside one is a search (`docs/architecture/http-api.md`,
 * "Methods"). A search leaves the trash out unless asked, and a trashed
 * conversation can still be opened, so the query asks.
 */
export function threadQueryFor(conversationId: number, date: string | null, term: string): string {
  const parts = [`in:#${conversationId}`, "trashed:any"];
  if (date !== null) parts.push(`date:${date}`);
  const trimmed = term.trim();
  if (trimmed) parts.push(quote(trimmed));
  return parts.join(" ");
}

/**
 * Where the thread scrolls once the messages it needs are on screen: to the
 * newest message at the bottom, or to one message. `seq` counts the jumps, so
 * a second jump to the same place scrolls again.
 */
export type Landing = {
  seq: number;
  to: "bottom" | { id: number; align: "center" | "start" };
};

/** Every loaded message once, oldest first. */
function flatten(pages: WindowPage[] | undefined): Message[] {
  if (!pages) return [];
  const seen = new Set<number>();
  const rows: Message[] = [];
  for (const page of pages) {
    for (const message of page.items) {
      if (seen.has(message.id)) continue;
      seen.add(message.id);
      rows.push(message);
    }
  }
  return rows;
}

/**
 * The conversation panel's messages (#1391). It opens at the newest message,
 * or at `openAt` when a search result opens it there (#313), and reads older
 * and newer pages as the person scrolls. Every jump (Newest, a year, a Find
 * match) reads the messages around its target and keeps reading outward from
 * there, so a conversation of any length is reachable.
 *
 * Find steps through the matches in place: the matches come from
 * `GET /v1/messages` with `in:#id`, newest first, and each one the person
 * steps to is a jump. Nothing is hidden.
 *
 * The view state belongs to one conversation. `MessageRoute` keys
 * `MessageView` by conversation id, so a new conversation starts with fresh
 * state rather than this hook resetting it.
 */
export function useConversationMessages(conversationId: number, openAt: number | null = null) {
  const cache = useRouteCache();
  const [start, setStart] = useState<WindowStart>(
    openAt === null ? { kind: "newest" } : { kind: "around", id: openAt },
  );
  const [landing, setLanding] = useState<Landing>({
    seq: 0,
    to: openAt === null ? "bottom" : { id: openAt, align: "center" },
  });
  /** The message a jump was for, drawn highlighted: a search result or a Find match. */
  const [highlightId, setHighlightId] = useState<number | null>(openAt);
  const [jumpError, setJumpError] = useState<Error | null>(null);

  const key = keys.conversations.messages(conversationId, startKey(start));
  const query = useRouteInfiniteQuery<WindowPage, WindowPageParam>(key, {
    initialPageParam: start,
    queryFn: ({ pageParam, signal }) => fetchWindowPage(conversationId, pageParam, signal),
    getPreviousPageParam: (oldest) => olderPageParam(oldest),
    getNextPageParam: (newest) => newerPageParam(newest),
    // A jump keeps the messages on screen until the ones around its target
    // land, instead of emptying the panel. Only within this conversation.
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[4] === String(conversationId) ? previous : undefined,
  });

  const messages = useMemo(() => flatten(query.data?.pages), [query.data?.pages]);
  const pages = query.data?.pages;
  const total = pages?.[pages.length - 1]?.total ?? 0;

  // The ids on screen, read by a jump without making it change on every page.
  const loadedIds = useRef(new Set<number>());
  loadedIds.current = useMemo(() => new Set(messages.map((m) => m.id)), [messages]);

  const land = useCallback((to: Landing["to"]) => {
    setLanding((l) => ({ seq: l.seq + 1, to }));
  }, []);

  /** Show one message: scroll to it when it is loaded, or read the messages around it. */
  const jumpToMessage = useCallback(
    (id: number, align: "center" | "start" = "center") => {
      setJumpError(null);
      if (!loadedIds.current.has(id)) setStart({ kind: "around", id });
      land({ id, align });
    },
    [land],
  );

  const hasNewer = query.hasNextPage;
  const jumpToNewest = () => {
    setJumpError(null);
    setHighlightId(null);
    if (hasNewer || messages.length === 0) setStart({ kind: "newest" });
    land("bottom");
  };

  /** Show the first message of `year`, or the first one after it when the year has none. */
  const jumpToYear = async (year: number) => {
    setJumpError(null);
    setHighlightId(null);
    const q = threadQueryFor(conversationId, `>=${year}`, "");
    try {
      const page = await cache.fetch(
        keys.conversations.find(conversationId, q, "date", 0, 1),
        (signal) => listMessages({ q, sort: "date", limit: 1 }, { signal }),
      );
      const first = page.items[0];
      if (first) jumpToMessage(first.id, "start");
    } catch (error) {
      setJumpError(error instanceof Error ? error : new Error(String(error)));
    }
  };

  // ── Find ──────────────────────────────────────────────────────────────
  const [findOpen, setFindOpen] = useState(false);
  const [findTerm, setFindTermState] = useState("");
  /** Which match is current, counted from the newest: 0 is the newest match. */
  const [matchIndex, setMatchIndex] = useState(0);

  const finding = findOpen && findTerm.trim().length > 0;
  const matchQuery = threadQueryFor(conversationId, null, findTerm);
  const matchOffset = Math.floor(matchIndex / PAGE_SIZE) * PAGE_SIZE;
  const matches = useRouteQuery(
    keys.conversations.find(conversationId, matchQuery, "-date", matchOffset, PAGE_SIZE),
    (signal) =>
      listMessages(
        { q: matchQuery, sort: "-date", offset: matchOffset, limit: PAGE_SIZE },
        { signal },
      ),
    {
      enabled: finding,
      // Stepping onto the next page of matches keeps the count on screen
      // while it loads; a new term starts again.
      placeholderData: (previous, previousQuery) =>
        previousQuery?.queryKey[5] === matchQuery ? previous : undefined,
    },
  );
  const matchTotal = finding ? (matches.data?.total ?? 0) : 0;
  const activeMatchId =
    finding && matches.data && !matches.isPlaceholderData
      ? (matches.data.items[matchIndex - matchOffset]?.id ?? null)
      : null;

  // Typing, or stepping to another match, jumps to it.
  const jumpedMatch = useRef<number | null>(null);
  useEffect(() => {
    if (activeMatchId === null || activeMatchId === jumpedMatch.current) return;
    jumpedMatch.current = activeMatchId;
    setHighlightId(activeMatchId);
    jumpToMessage(activeMatchId);
  }, [activeMatchId, jumpToMessage]);

  const setFindTerm = (term: string) => {
    setFindTermState(term);
    setMatchIndex(0);
  };

  /** ▼: the next newer match, or round to the oldest. */
  const nextMatch = () => {
    if (matchTotal === 0) return;
    setMatchIndex((i) => (i - 1 + matchTotal) % matchTotal);
  };

  /** ▲: the next older match, or round to the newest. */
  const prevMatch = () => {
    if (matchTotal === 0) return;
    setMatchIndex((i) => (i + 1) % matchTotal);
  };

  /** ✕: Find closes where the person is; the thread stays where it is. */
  const closeFind = () => {
    setFindOpen(false);
    setFindTermState("");
    setMatchIndex(0);
    setHighlightId(null);
    jumpedMatch.current = null;
  };

  return {
    messages,
    total,
    /** Nothing has loaded yet. */
    loading: query.isPending,
    error: query.error ?? jumpError,
    /** The messages on screen are the last place's, while a jump loads. */
    jumping: query.isPlaceholderData,
    hasOlder: query.hasPreviousPage,
    hasNewer,
    loadingOlder: query.isFetchingPreviousPage,
    loadingNewer: query.isFetchingNextPage,
    loadOlder: () => {
      if (query.hasPreviousPage && !query.isFetchingPreviousPage) void query.fetchPreviousPage();
    },
    loadNewer: () => {
      if (query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage();
    },
    landing,
    highlightId,
    jumpToNewest,
    jumpToYear,
    jumpToMessage,
    find: {
      open: findOpen,
      openFind: () => setFindOpen(true),
      close: closeFind,
      term: findTerm,
      setTerm: setFindTerm,
      finding,
      total: matchTotal,
      /** Zero-based, from the newest match. */
      position: matchIndex,
      searching: finding && matches.isFetching,
      nextMatch,
      prevMatch,
    },
  };
}
