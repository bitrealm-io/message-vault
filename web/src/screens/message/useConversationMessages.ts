import { useState } from "react";
import { quote } from "../../lib/searchQuery";
import { yearIn } from "../../lib/timeZone";
import type { Message } from "../../lib/types";
import { listConversationMessages, listMessages } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";

/** Page size for the thread, whatever it is showing: all years, one year, or a find. */
export const PAGE_SIZE = 50;

/** One page's worth of conversation messages, as the hook hands them to a screen. */
type MessagesResult = { items: Message[]; total: number };

/**
 * Calendar years covered by a conversation's first and last message instants,
 * read in the account's `zone`: the same rule `date:2024` uses, so every chip
 * names a year that has messages in it.
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

/** Short label for a backup source shown in the message footer. */
export function displaySourceLabel(source: string): string {
  const token = source.trim().toLowerCase();
  if (token === "sms-backup-restore") return "SMS/MMS";
  if (token === "whatsapp") return "WhatsApp";
  return source.trim() || "unknown";
}

/**
 * Footer count line: the window of the page the person is on, named for what
 * the thread is showing. Every view pages the same way; a year is no longer
 * loaded in full (#323).
 */
export function buildFooterLabel(
  activeYear: number | null,
  total: number,
  offset: number,
  finding = false,
): string {
  const subject = finding ? "Matches" : activeYear === null ? "Messages" : `${activeYear}:`;
  if (total === 0) return `${subject} 0 of 0`;
  return `${subject} ${offset + 1}–${Math.min(offset + PAGE_SIZE, total)} of ${total}`;
}

/**
 * The search-language query a narrowed thread compiles to: the conversation
 * by id, whether or not it is in the trash, the active year when one is
 * chosen, and the typed find term, when there is one, as free text. Runs on
 * `GET /v1/messages`, so it reaches every message in the conversation, not
 * the page in hand (#313). Opening a conversation takes no filter; a year or
 * a find inside one is a search (`docs/architecture/http-api.md`, "Methods").
 * A search leaves the trash out unless asked, and a trashed conversation can
 * still be opened, so the query asks.
 */
export function threadQueryFor(
  conversationId: number,
  activeYear: number | null,
  term: string,
): string {
  const parts = [`in:#${conversationId}`, "trashed:any"];
  if (activeYear !== null) parts.push(`date:${activeYear}`);
  const trimmed = term.trim();
  if (trimmed) parts.push(quote(trimmed));
  return parts.join(" ");
}

/**
 * What a cache key says the thread is looking at: the list under
 * `conversations` (messages or find) and the conversation id. Two keys with
 * the same scope show the same kind of rows for the same thread, so the
 * previous page may stand in while the next one loads; anything else (a new
 * conversation, a find replacing the thread) must not.
 */
function threadScope(queryKey: readonly unknown[] | undefined): string | null {
  if (!queryKey) return null;
  const at = queryKey.indexOf("conversations");
  if (at < 0) return null;
  return `${String(queryKey[at + 1])}:${String(queryKey[at + 2])}`;
}

/**
 * Load messages for one conversation a page at a time: the whole thread, one
 * calendar year of it, or the messages matching the find box.
 */
export function useConversationMessages(conversationId: number) {
  /** `offset`, `activeYear`, `findTerm` and `activeMatch` are view state, not
   * server state — a screen's choice of what to look at, not anything the
   * vault owns. The messages themselves come from `useVaultQuery` below. */
  const [offset, setOffset] = useState(0);
  /** `null` = all years. Otherwise the thread (or the find) is narrowed to that calendar year. */
  const [activeYear, setActiveYear] = useState<number | null>(null);
  const [findTerm, setFindTermState] = useState("");
  const [activeMatch, setActiveMatch] = useState(0);

  // A new conversation starts back at page one, with no year filter or find
  // term carried over from the last one. Adjusted during render, per React's
  // own guidance for resetting state on a prop change, rather than in an
  // effect that would otherwise run one render late.
  const [renderedConversationId, setRenderedConversationId] = useState(conversationId);
  if (conversationId !== renderedConversationId) {
    setRenderedConversationId(conversationId);
    setOffset(0);
    setActiveYear(null);
    setFindTermState("");
    setActiveMatch(0);
  }

  const finding = findTerm.trim().length > 0;
  // The whole thread is the conversation read by id; a year or a find is a
  // search scoped to it.
  const searching = finding || activeYear !== null;
  const threadQuery = searching ? threadQueryFor(conversationId, activeYear, findTerm) : "";

  const key = searching
    ? keys.conversations.find(conversationId, threadQuery, offset, PAGE_SIZE)
    : keys.conversations.messages(conversationId, { offset, limit: PAGE_SIZE });

  const query = useVaultQuery<MessagesResult>(
    key,
    (signal) =>
      searching
        ? listMessages({ q: threadQuery, offset, limit: PAGE_SIZE }, { signal })
        : listConversationMessages(conversationId, { offset, limit: PAGE_SIZE }, { signal }),
    {
      // Turning a page keeps the current one on screen until the next lands,
      // instead of flashing "0 of 0" and disabling both pager buttons
      // (#326). Scoped to the same thread and the same kind of rows, so one
      // conversation's page never stands in for another's.
      placeholderData: (previous, previousQuery) =>
        threadScope(previousQuery?.queryKey) === threadScope([...key]) ? previous : undefined,
    },
  );

  const messages = query.data?.items ?? [];
  const total = query.data?.total ?? 0;

  const fetchConversationPage = (newOffset: number) => {
    setOffset(newOffset);
    setActiveMatch(0);
  };

  const setFindTerm = (term: string) => {
    setFindTermState(term);
    setOffset(0);
    setActiveMatch(0);
  };

  const selectAllYears = () => {
    setActiveYear(null);
    setOffset(0);
    setActiveMatch(0);
  };

  const selectYear = (year: number) => {
    if (activeYear === year) {
      selectAllYears();
      return;
    }
    setActiveYear(year);
    setOffset(0);
    setActiveMatch(0);
  };

  return {
    messages,
    total,
    offset,
    activeYear,
    findTerm,
    setFindTerm,
    finding,
    activeMatch,
    setActiveMatch,
    loading: query.isLoading,
    /** A cached page is being revalidated in the background. */
    refreshing: query.isFetching && !query.isLoading,
    fetchConversationPage,
    selectAllYears,
    selectYear,
    data: query.data,
    error: query.error,
    isLoading: query.isLoading,
  };
}
