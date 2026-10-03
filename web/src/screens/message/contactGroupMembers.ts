import { useMemo } from "react";
import { phonesMatch } from "../../lib/phoneTokens";
import type { Conversation, Participant } from "../../lib/types";
import { useAccountProfile } from "../../lib/useAccountProfile";

/**
 * The contacts "Make a Contact Group from these people" would add: everyone
 * in a group chat who has a contact, but the account owner. Empty when the
 * conversation is not a group or fewer than two such people are in it, and
 * the conversation panel's menu then does not offer it.
 */
export function useContactGroupMembers(conversation: Conversation): number[] {
  const { profile } = useAccountProfile();
  const memberIds = useMemo(
    () => memberContactIds(conversation.participants, profile?.phones ?? [], profile?.emails ?? []),
    [conversation.participants, profile?.phones, profile?.emails],
  );
  return conversation.is_group && memberIds.length >= 2 ? memberIds : [];
}

/** The distinct contact ids of everyone in the chat except the account owner. */
function memberContactIds(
  participants: readonly Participant[],
  ownerPhones: readonly string[],
  ownerEmails: readonly string[],
): number[] {
  const owner = (handle: string | null | undefined): boolean => {
    if (!handle) return false;
    if (handle.includes("@")) {
      const wanted = handle.trim().toLowerCase();
      return ownerEmails.some((e) => e.trim().toLowerCase() === wanted);
    }
    return ownerPhones.some((p) => phonesMatch(p, handle));
  };
  const ids: number[] = [];
  for (const p of participants) {
    if (p.contact_id == null || owner(p.identity)) continue;
    if (!ids.includes(p.contact_id)) ids.push(p.contact_id);
  }
  return ids;
}
