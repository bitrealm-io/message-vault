import type { components } from "../../lib/vaultApi.types";

/** One identity as the vault lists it, with the messages it takes part in. */
export type Identity = components["schemas"]["AccountIdentity"];

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
