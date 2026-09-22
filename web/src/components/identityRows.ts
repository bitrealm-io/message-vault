import type { SortDescriptor } from "react-aria-components";
import { formatHandleServiceLabel } from "../lib/handleService";

/**
 * One identity as either screen shows it: the address, the service it is on,
 * and what it takes part in. The contact drawer maps a contact's identity onto
 * this and the Profile tab maps an account's, so the two tables are one.
 */
export type IdentityRow = {
  handle: string;
  service: string | null;
  /** When the oldest message the identity takes part in was sent, or null. */
  start_date: string | null;
  /** When the newest was sent, or null. */
  end_date: string | null;
  /** Direct and group conversations the identity takes part in. */
  conversations: number;
  direct_messages: number;
  group_messages: number;
};

const SORT_COLUMNS = [
  "service",
  "handle",
  "start_date",
  "end_date",
  "conversations",
  "direct_messages",
  "group_messages",
] as const;
type SortColumn = (typeof SORT_COLUMNS)[number];

function sortKey(row: IdentityRow, column: SortColumn): string | number {
  switch (column) {
    case "service":
      return formatHandleServiceLabel(row.handle, row.service).toLowerCase();
    case "handle":
      return row.handle.toLowerCase();
    case "start_date":
      return row.start_date ?? "";
    case "end_date":
      return row.end_date ?? "";
    case "conversations":
      return row.conversations;
    case "direct_messages":
      return row.direct_messages;
    case "group_messages":
      return row.group_messages;
  }
}

/** The rows in the order the header asks for; unsorted, as the caller listed them. */
export function sortIdentityRows(
  rows: readonly IdentityRow[],
  sort: SortDescriptor | null,
): IdentityRow[] {
  const column = SORT_COLUMNS.find((c) => c === sort?.column);
  if (!sort || !column) return [...rows];
  const dir = sort.direction === "descending" ? -1 : 1;
  return [...rows].sort((a, b) => {
    const av = sortKey(a, column);
    const bv = sortKey(b, column);
    if (av < bv) return -dir;
    if (av > bv) return dir;
    return a.handle.localeCompare(b.handle);
  });
}

/** The earliest, the latest, and the sums across every row, for the Summary row. */
export function identityTotals(rows: readonly IdentityRow[]): IdentityRow {
  let start: string | null = null;
  let end: string | null = null;
  let conversations = 0;
  let direct = 0;
  let group = 0;
  for (const row of rows) {
    if (row.start_date && (!start || row.start_date < start)) start = row.start_date;
    if (row.end_date && (!end || row.end_date > end)) end = row.end_date;
    conversations += row.conversations;
    direct += row.direct_messages;
    group += row.group_messages;
  }
  return {
    handle: "",
    service: null,
    start_date: start,
    end_date: end,
    conversations,
    direct_messages: direct,
    group_messages: group,
  };
}
