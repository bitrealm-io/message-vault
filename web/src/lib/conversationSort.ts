import type { SortOrder } from "../components/SortMenu";

/**
 * How the conversation list is ordered.
 *
 * Unlike contacts, this is not a client-side sort. The conversation list pages
 * in as you scroll, so reordering the rows already loaded would reorder a
 * fraction of the list and read as a bug. Both values go to the server, which
 * orders the whole result set — see `list_conversations_sorted`.
 */
export type ConversationSort = "date" | "messages";

export interface ConversationSortState {
  sort: ConversationSort;
  order: SortOrder;
}

/** Newest activity first — what the list showed before it was configurable. */
export const DEFAULT_CONVERSATION_SORT = {
  sort: "date",
  order: "desc",
} as const satisfies ConversationSortState;

const STORAGE_KEY = "conversationSort:v1";

function isSort(value: unknown): value is ConversationSort {
  return value === "date" || value === "messages";
}

function isOrder(value: unknown): value is SortOrder {
  return value === "asc" || value === "desc";
}

export function loadConversationSort(): ConversationSortState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_CONVERSATION_SORT };
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) {
      return { ...DEFAULT_CONVERSATION_SORT };
    }
    const rec = parsed as Record<string, unknown>;
    return {
      sort: isSort(rec.sort) ? rec.sort : DEFAULT_CONVERSATION_SORT.sort,
      order: isOrder(rec.order) ? rec.order : DEFAULT_CONVERSATION_SORT.order,
    };
  } catch {
    return { ...DEFAULT_CONVERSATION_SORT };
  }
}

export function saveConversationSort(state: ConversationSortState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // private browsing / quota
  }
}
