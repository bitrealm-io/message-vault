import { contactLabelText } from "./contactLabel";

/** The two name fields the list can order by; each also gives the A–Z section letters. */
export type ContactNameSort = "first" | "last";
/**
 * How the contact list is ordered: a name field, or when the vault last heard
 * from the contact (`last_heard_at`, the vault's `sort=last_heard`).
 */
export type ContactSort = ContactNameSort | "lastHeard";
export type ContactSortOrder = "asc" | "desc";

export interface ContactSortState {
  sort: ContactSort;
  order: ContactSortOrder;
}

export const DEFAULT_CONTACT_SORT = {
  sort: "last",
  order: "asc",
} as const satisfies ContactSortState;

const STORAGE_KEY = "contactSort:v1";

/** The row fields the sort reads. */
export interface SortableContact {
  name: string;
  handles?: readonly string[];
  last_heard_at?: string | null;
}

export function isNameSort(sort: ContactSort): sort is ContactNameSort {
  return sort === "first" || sort === "last";
}

/**
 * The order a field starts in when picked: A to Z for a name, newest first
 * for last heard. Picking the field that is already active leaves the order
 * alone, so the menu's Order section still applies to it.
 */
export function withSortField(prev: ContactSortState, sort: ContactSort): ContactSortState {
  if (sort === prev.sort) return prev;
  return { sort, order: sort === "lastHeard" ? "desc" : "asc" };
}

/** First word and last word of a display name. A single word is used for both. */
export function splitContactName(name: string): { first: string; last: string } {
  const trimmed = name.trim();
  if (!trimmed) return { first: "", last: "" };

  if (trimmed.includes(",")) {
    const [lastPart, firstPart] = trimmed.split(",").map((s) => s.trim());
    const last = lastPart || firstPart || "";
    const first = firstPart || lastPart || "";
    return { first, last };
  }

  const parts = trimmed.split(/\s+/).filter(Boolean);
  const only = parts[0] ?? "";
  if (parts.length <= 1) return { first: only, last: only };
  return { first: only, last: parts[parts.length - 1] ?? only };
}

/** A–Z from the active name field, or `#` for numbers and symbols. */
export function contactSortLetter(name: string, sort: ContactNameSort): string {
  const parts = splitContactName(name);
  const src = sort === "first" ? parts.first : parts.last;
  const ch = src.charAt(0).toUpperCase();
  return ch >= "A" && ch <= "Z" ? ch : "#";
}

/** Split an already-sorted list into letter groups. Order of groups is kept. */
export function groupByLetter<T>(
  items: readonly T[],
  letterOf: (item: T) => string,
): ReadonlyArray<readonly [string, readonly T[]]> {
  const groups: Array<[string, T[]]> = [];
  for (const item of items) {
    const letter = letterOf(item);
    const last = groups[groups.length - 1];
    if (last && last[0] === letter) {
      last[1].push(item);
    } else {
      groups.push([letter, [item]]);
    }
  }
  return groups;
}

export function compareContactsByName(
  a: string,
  b: string,
  sort: ContactNameSort,
  order: ContactSortOrder,
): number {
  const pa = splitContactName(a);
  const pb = splitContactName(b);
  const primary = sort === "first" ? "first" : "last";
  const secondary = sort === "first" ? "last" : "first";
  let cmp = pa[primary].localeCompare(pb[primary], undefined, {
    sensitivity: "base",
  });
  if (cmp === 0) {
    cmp = pa[secondary].localeCompare(pb[secondary], undefined, {
      sensitivity: "base",
    });
  }
  return order === "desc" ? -cmp : cmp;
}

/**
 * By when the vault last heard from each contact. A contact it never heard
 * from has no date and goes last in either direction, the way the vault
 * orders `sort=last_heard`; the timestamps are RFC 3339 in UTC, so string
 * order is time order.
 */
export function compareContactsByLastHeard(
  a: string | null | undefined,
  b: string | null | undefined,
  order: ContactSortOrder,
): number {
  if (!a && !b) return 0;
  if (!a) return 1;
  if (!b) return -1;
  const cmp = a < b ? -1 : a > b ? 1 : 0;
  return order === "desc" ? -cmp : cmp;
}

/** The list's comparator for `state`; ties on last heard fall back to the name, A to Z. */
export function compareContacts(a: SortableContact, b: SortableContact, state: ContactSortState) {
  const labelA = contactLabelText(a.name, a.handles);
  const labelB = contactLabelText(b.name, b.handles);
  if (isNameSort(state.sort)) {
    return compareContactsByName(labelA, labelB, state.sort, state.order);
  }
  return (
    compareContactsByLastHeard(a.last_heard_at, b.last_heard_at, state.order) ||
    compareContactsByName(labelA, labelB, "last", "asc")
  );
}

function isSort(value: unknown): value is ContactSort {
  return value === "first" || value === "last" || value === "lastHeard";
}

function isSortOrder(value: unknown): value is ContactSortOrder {
  return value === "asc" || value === "desc";
}

export function loadContactSort(): ContactSortState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_CONTACT_SORT };
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) {
      return { ...DEFAULT_CONTACT_SORT };
    }
    const rec = parsed as Record<string, unknown>;
    return {
      sort: isSort(rec.sort) ? rec.sort : DEFAULT_CONTACT_SORT.sort,
      order: isSortOrder(rec.order) ? rec.order : DEFAULT_CONTACT_SORT.order,
    };
  } catch {
    return { ...DEFAULT_CONTACT_SORT };
  }
}

export function saveContactSort(state: ContactSortState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // private browsing / quota
  }
}
