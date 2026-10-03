import { formatClockTime } from "../../lib/formatDate";
import { useTimeZone } from "../../lib/timeZone";
import MessageAttachments from "../MessageAttachments";
import {
  bubbleBody,
  ChatBubbleRow,
  isGroupConversation,
  type MessageBubbleProps,
  senderName,
  tapbackGroups,
} from "./chatBubbleShared";

export default function ImessageBubble({
  message,
  highlight,
  isActive,
  showSender,
  onAttachmentClick,
}: MessageBubbleProps) {
  const time = formatClockTime(message.timestamp, useTimeZone());
  const mine = message.is_from_me;
  const group = isGroupConversation(message);
  const body = (message.text || "").trim();
  const hasAttachments = message.attachments.length > 0;
  const tapbacks = tapbackGroups(message);
  const hasTapbacks = tapbacks.length > 0;

  const footer =
    hasAttachments || hasTapbacks ? (
      <>
        <MessageAttachments message={message} onAttachmentClick={onAttachmentClick} />
        {hasTapbacks ? (
          <div className={`flex flex-wrap gap-1 ${hasAttachments ? "mt-1" : "mt-0"}`}>
            {tapbacks.map((t) => (
              <span
                key={t.emoji}
                title={t.senderNames.join(", ")}
                className="rounded-full border border-border bg-elevated px-[0.35rem] py-[0.1rem] text-[0.75rem] text-text"
              >
                {t.emoji} {t.count}
              </span>
            ))}
          </div>
        ) : null}
      </>
    ) : undefined;

  return (
    <ChatBubbleRow
      messageId={String(message.id)}
      mine={mine}
      isActive={isActive}
      palette="imessage"
      showSender={!mine && (showSender ?? group)}
      senderLabel={senderName(message)}
      timeLabel={time}
      footer={footer}
    >
      {bubbleBody(body, highlight)}
    </ChatBubbleRow>
  );
}
