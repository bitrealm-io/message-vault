/** What a conversation's name is made from, on either list's row. */
export type ConversationNameParts = {
  /** The conversation's own title, when the export gave one. */
  title: string | null | undefined;
  /** Whether the conversation is a group, as the server reads `conversation_type`. */
  isGroup: boolean;
  participants: readonly { name: string }[];
};

/**
 * A conversation's name as the conversation list shows it: its title, else
 * the one other person in a one-to-one conversation, else every participant,
 * else "(unknown)". The Messages list names a message's conversation with
 * this too, so the two lists never name one conversation two ways.
 */
export function conversationName({ title, isGroup, participants }: ConversationNameParts): string {
  const trimmed = title?.trim();
  if (trimmed) return trimmed;
  const names = participants.map((p) => p.name);
  if (!isGroup) return names[0] ?? "(unknown)";
  return names.length > 0 ? names.join(", ") : "(unknown)";
}

/** Whether a `conversation_type` is a group, as the server reads it: ignoring case. */
export function isGroupType(conversationType: string): boolean {
  return conversationType.toLowerCase() === "group";
}
