import { formatHandleServiceLabel } from "../../lib/handleService";
import type { components } from "../../lib/vaultApi.types";

/** One identity as the vault lists it, with the messages it takes part in. */
export type Identity = components["schemas"]["AccountIdentity"];

/** The columns the Identities table sorts by. */
export const SORT_COLUMNS = ["service", "handle", "in_use"] as const;
export type SortColumn = (typeof SORT_COLUMNS)[number];

/** "12 direct messages and 30 group messages", "1 direct message", or null when there are none. */
export function messagesPhrase(identity: Identity): string | null {
  const parts: string[] = [];
  const count = (n: number, kind: string) =>
    `${n.toLocaleString()} ${kind} message${n === 1 ? "" : "s"}`;
  if (identity.direct_messages > 0) parts.push(count(identity.direct_messages, "direct"));
  if (identity.group_messages > 0) parts.push(count(identity.group_messages, "group"));
  return parts.length === 0 ? null : parts.join(" and ");
}

/** What the confirm dialog says an identity is tied to. */
export function removeBody(identity: Identity): string {
  const phrase = messagesPhrase(identity);
  return phrase
    ? `${phrase} will no longer be associated with this account.`
    : `${identity.handle} takes part in no messages. It will no longer count as this account's own.`;
}

/** An identity is in use when it takes part in any message. */
export function inUse(identity: Identity): boolean {
  return identity.direct_messages > 0 || identity.group_messages > 0;
}

/** The identities in the order the table's header asks for; unsorted, as the vault lists them. */
export function sortIdentities(
  rows: readonly Identity[],
  sort: { column: SortColumn; direction: "ascending" | "descending" } | null,
): Identity[] {
  if (!sort) return [...rows];
  const dir = sort.direction === "descending" ? -1 : 1;
  const compare = (a: Identity, b: Identity): number => {
    switch (sort.column) {
      case "service":
        return formatHandleServiceLabel(a.handle, a.service).localeCompare(
          formatHandleServiceLabel(b.handle, b.service),
        );
      case "handle":
        return a.handle.localeCompare(b.handle);
      case "in_use":
        return Number(inUse(a)) - Number(inUse(b));
    }
  };
  return [...rows].sort((a, b) => dir * compare(a, b));
}
