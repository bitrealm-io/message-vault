import type { Message, MessageConversation } from "./types";

/**
 * A conversation's name as the conversation list shows it: its title, else
 * the one other person in a one-to-one conversation, else every participant.
 */
export function messageConversationName(conversation: MessageConversation): string {
  const title = conversation.group_title?.trim();
  if (title) return title;
  const names = conversation.participants.map((p) => p.name);
  if (conversation.conversation_type !== "group") return names[0] ?? "(unknown)";
  return names.length > 0 ? names.join(", ") : "(unknown)";
}

/**
 * Who sent a message: "You" for one the account sent, else the participant
 * whose identity sent it, by the name the conversation gives them, else the
 * identity itself.
 */
export function messageSenderName(message: Message): string {
  if (message.is_from_me) return "You";
  const sender = message.sender;
  if (!sender) return "Unknown";
  const participant = message.conversation.participants.find((p) => p.handle === sender);
  return participant?.name ?? sender;
}

/** The text a row shows for a message: its text, or the names of its attachments when it has none. */
export function messageRowText(message: Message): string {
  const text = message.text?.trim();
  if (text) return text;
  return message.attachments
    .map((a) => a.original_name?.trim())
    .filter((name): name is string => Boolean(name))
    .join(", ");
}

/** The Messages list's total: "1 message", "12,408 messages". */
export function messageCount(total: number): string {
  return total === 1 ? "1 message" : `${total.toLocaleString()} messages`;
}
