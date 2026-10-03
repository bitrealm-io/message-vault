import type { Conversation } from "./types";

/**
 * Short service label for a conversation.
 *
 * iMessage and SMS/MMS are the same thing to someone reading their messages — a
 * text message — and which transport carried it is not what the label is for.
 * Anything else (WhatsApp, say) keeps its own name.
 */
export function formatServiceLabel(service: string | null | undefined): string | null {
  const s = (service ?? "").trim();
  if (!s || s.toLowerCase() === "unknown") return null;
  const lower = s.toLowerCase();
  const texting = ["imessage", "ios", "sms/mms", "sms", "mms"];
  if (texting.includes(lower) || lower.includes("sms")) return "Text Message";
  return s;
}

/**
 * The service label the conversation list and the conversation panel show:
 * the conversation's own, or for a one-to-one conversation without one, its
 * participant's.
 */
export function conversationServiceLabel(conv: Conversation): string | null {
  const fromConv = formatServiceLabel(conv.service);
  if (fromConv || conv.is_group) return fromConv;
  const p = conv.participants[0];
  return p ? formatServiceLabel(p.service) : null;
}
