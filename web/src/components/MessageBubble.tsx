import { memo } from "react";
import type { Message, MessageAttachment } from "../lib/types";
import DiscordBubble from "./messages/DiscordBubble";
import ImessageBubble from "./messages/ImessageBubble";
import InstagramBubble from "./messages/InstagramBubble";
import SmsBubble from "./messages/SmsBubble";
import WhatsAppBubble from "./messages/WhatsAppBubble";

export type AttachmentClickHandler = (attachment: MessageAttachment, source: string) => void;

function normalizeToken(value: string | null | undefined): string {
  return (value || "").trim().toLowerCase();
}

/** Pick bubble style from each message's transport and import source. */
function resolveBubbleKind(
  message: Message,
): "imessage" | "discord" | "whatsapp" | "instagram" | "sms" {
  const service = normalizeToken(message.service);
  const source = normalizeToken(message.source);

  if (
    service === "imessage" ||
    service === "ios" ||
    service.includes("imessage") ||
    source === "imessage" ||
    source.startsWith("imessage") ||
    source.includes("iphone") ||
    source.includes("macos")
  ) {
    return "imessage";
  }
  if (service === "discord" || source.includes("discord")) return "discord";
  if (service === "whatsapp" || source.includes("whatsapp")) return "whatsapp";
  if (service === "instagram" || source.includes("instagram")) return "instagram";
  return "sms";
}

function MessageBubble({
  message,
  highlight,
  isActive,
  showSender,
  onAttachmentClick,
}: {
  message: Message;
  highlight?: string;
  isActive?: boolean;
  /** Whether a received message carries its sender's name; by default, in a group. */
  showSender?: boolean;
  onAttachmentClick?: AttachmentClickHandler;
}) {
  const props = { message, highlight, isActive, showSender, onAttachmentClick };

  switch (resolveBubbleKind(message)) {
    case "imessage":
      return <ImessageBubble {...props} />;
    case "discord":
      return <DiscordBubble {...props} />;
    case "whatsapp":
      return <WhatsAppBubble {...props} />;
    case "instagram":
      return <InstagramBubble {...props} />;
    default:
      return <SmsBubble {...props} />;
  }
}

/**
 * A thread renders every message it has loaded, so stepping through find matches
 * would otherwise re-render the whole list to move one highlight. `onAttachmentClick`
 * is a stable callback in the one screen that passes it, so this holds.
 */
export default memo(MessageBubble);
