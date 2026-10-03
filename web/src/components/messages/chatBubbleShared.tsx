import type { CSSProperties, ReactNode } from "react";
import { highlightText } from "../../lib/highlightText";
import { useTimeZone } from "../../lib/timeZone";
import type { Message, MessageAttachment, MessageTapback } from "../../lib/types";

type BubblePalette = "imessage" | "sms";

/** Props every per-service message renderer accepts. */
export type MessageBubbleProps = {
  message: Message;
  highlight?: string;
  isActive?: boolean;
  /**
   * Whether a received message carries its sender's name. The conversation
   * panel names the sender only at the start of a run in a group (#1391);
   * left out, every received message in a group is named.
   */
  showSender?: boolean;
  onAttachmentClick?: (attachment: MessageAttachment, source: string) => void;
};

/**
 * Timestamp shown under a bubble or beside a flat-row sender, read in `zone`.
 *
 * `timestamp` is the UTC instant the server stores; `zone` is the account's
 * (`useTimeZone`), which is what makes it the clock reading the person saw.
 */
export function formatMessageTime(timestamp: string, zone: string, withYear = false): string {
  return new Date(timestamp).toLocaleString([], {
    timeZone: zone,
    month: "short",
    day: "numeric",
    ...(withYear ? { year: "numeric" as const } : {}),
    hour: "numeric",
    minute: "2-digit",
  });
}

/** Message text with search matches marked, or nothing when the body is empty. */
export function bubbleBody(
  body: string,
  highlight: string | undefined,
): ReactNode[] | string | undefined {
  if (!body) return undefined;
  return highlight ? highlightText(body, highlight) : body;
}

export function senderName(m: Message): string {
  if (m.is_from_me) return "Me";
  if (m.sender) {
    const p = m.conversation.participants.find((x) => x.identity === m.sender);
    return p ? p.name : m.sender;
  }
  const p = m.conversation.participants[0];
  return p ? p.name : "Unknown";
}

export function isGroupConversation(m: Message): boolean {
  return m.conversation.conversation_type === "group" || m.conversation.participants.length > 1;
}

/**
 * iMessage's fixed tapback kinds carry no emoji of their own (the export
 * sends `emoji: null` for them) — the client renders the emoji instead.
 * A tapback with its own `emoji` (Discord, say) always wins over this.
 */
const TAPBACK_KIND_EMOJI: Record<string, string> = {
  loved: "❤️",
  liked: "👍",
  disliked: "👎",
  laughed: "😂",
  emphasized: "‼️",
  questioned: "❓",
  // The exporter's kind vocabulary ends `emoji|sticker`. An `emoji` tapback
  // carries its own character in `emoji`; a `sticker` one carries nothing, so
  // without an entry here the badge rendered the literal word "sticker".
  sticker: "🖼️",
};

/** Who left a tapback: "Me" for the account owner, else the matching participant's name. */
function tapbackSenderName(m: Message, t: MessageTapback): string {
  if (t.is_from_me) return "Me";
  if (t.sender) {
    const p = m.conversation.participants.find((x) => x.identity === t.sender);
    if (p) return p.name;
    return t.sender;
  }
  return "Someone";
}

type TapbackGroup = {
  /** The emoji to show — the tapback's own, or the fixed kind's, or the raw kind as a last resort. */
  emoji: string;
  count: number;
  senderNames: string[];
};

/** This message's tapbacks, grouped by the emoji they display, each with who sent it. */
export function tapbackGroups(m: Message): TapbackGroup[] {
  const groups = new Map<string, TapbackGroup>();
  for (const t of m.tapbacks) {
    const emoji = t.emoji || TAPBACK_KIND_EMOJI[t.kind] || t.kind;
    const senderName = tapbackSenderName(m, t);
    const existing = groups.get(emoji);
    if (existing) {
      existing.count += 1;
      existing.senderNames.push(senderName);
    } else {
      groups.set(emoji, { emoji, count: 1, senderNames: [senderName] });
    }
  }
  return [...groups.values()];
}

/** Bubble fill/text color per palette (theme vars switch with data-theme). */
function bubbleColorClasses(palette: BubblePalette, mine: boolean): string {
  if (mine) {
    return palette === "imessage"
      ? "bg-[var(--imessage-sent)] text-[var(--imessage-sent-text)]"
      : "bg-[var(--sms-sent)] text-[var(--sms-sent-text)]";
  }
  return "bg-[var(--bubble-received)] text-[var(--bubble-received-text)]";
}

/** Sender-label color for the palette (same sent color as the bubble). */
function senderColorClass(palette: BubblePalette): string {
  return palette === "imessage" ? "text-[var(--imessage-sent)]" : "text-[var(--sms-sent)]";
}

/** Chat-row chrome: aligned bubble, optional sender label, timestamp under bubble. */
export function ChatBubbleRow({
  messageId,
  mine,
  isActive,
  palette,
  showSender,
  senderLabel,
  timeLabel,
  meta,
  children,
  footer,
}: {
  messageId: string;
  mine: boolean;
  isActive?: boolean;
  palette: BubblePalette;
  showSender?: boolean;
  senderLabel?: string;
  timeLabel: string;
  meta?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
}) {
  const radius = mine ? "rounded-[18px] rounded-br-[4px]" : "rounded-[18px] rounded-bl-[4px]";
  const hasBubble = children != null && children !== false && children !== "";

  return (
    <div
      id={`msg-${messageId}`}
      className={`flex flex-col px-4 py-[0.2rem] ${
        mine ? "mb-[0.15rem] items-end" : "mb-[0.4rem] items-start"
      } ${isActive ? "bg-search-active" : "bg-transparent"}`}
    >
      {showSender && senderLabel ? (
        <div
          className={`mb-[0.15rem] max-w-[min(78%,34rem)] px-[0.35rem] text-[0.75rem] font-semibold ${senderColorClass(palette)}`}
        >
          {senderLabel}
        </div>
      ) : null}

      {hasBubble ? (
        <div
          className={`${radius} ${bubbleColorClasses(palette, mine)} max-w-[min(78%,34rem)] whitespace-pre-wrap break-words px-[0.7rem] py-[0.45rem] text-[0.9375rem] leading-[1.35] ${
            mine ? "" : "shadow-[0_0.5px_0_rgb(0_0_0/0.06)]"
          }`}
        >
          {children}
        </div>
      ) : null}

      {footer ? (
        <div
          className={`max-w-[min(78%,34rem)] ${
            hasBubble ? "mt-[0.2rem]" : "mt-0"
          } ${mine ? "self-end" : "self-start"}`}
        >
          {footer}
        </div>
      ) : null}

      <div className="mt-[0.15rem] flex items-center gap-[0.4rem] px-[0.35rem] text-[0.688rem] text-muted">
        <span>{timeLabel}</span>
        {meta}
      </div>
    </div>
  );
}

/** Full-width row chrome for services rendered as flat rows rather than bubbles. */
export function ServiceRow({
  messageId,
  isActive,
  children,
}: {
  messageId: string;
  isActive?: boolean;
  children: ReactNode;
}) {
  return (
    <div
      id={`msg-${messageId}`}
      className={`border-b border-border px-6 py-2 ${
        isActive ? "bg-search-active" : "bg-transparent"
      }`}
    >
      {children}
    </div>
  );
}

/**
 * Shared branded-service row: ServiceRow + sender/time header.
 * Color and header alignment stay per-service; body is `children`.
 */
export function ServiceBubbleShell({
  message,
  isActive,
  senderClassName,
  senderStyle,
  timeClassName = "text-[0.75rem] text-muted",
  headerAlignClassName,
  children,
}: {
  message: Message;
  isActive?: boolean;
  senderClassName?: string;
  senderStyle?: CSSProperties;
  timeClassName?: string;
  /** Extra flex alignment on the header row (e.g. `items-center`). */
  headerAlignClassName?: string;
  children: ReactNode;
}) {
  const mine = message.is_from_me;
  const zone = useTimeZone();
  return (
    <ServiceRow messageId={String(message.id)} isActive={isActive}>
      <div
        className={`mb-1 flex gap-2 ${headerAlignClassName ?? ""} ${
          mine ? "justify-end" : "justify-start"
        }`}
      >
        <span
          className={`text-[0.75rem] font-semibold ${senderClassName ?? ""}`}
          style={senderStyle}
        >
          {senderName(message)}
        </span>
        <span className={timeClassName}>{formatMessageTime(message.timestamp, zone)}</span>
      </div>
      {children}
    </ServiceRow>
  );
}

/** Body text of a flat service row, aligned by author and search-highlighted. */
export function ServiceMessageText({
  text,
  highlight,
  mine,
}: {
  text: string;
  highlight?: string;
  mine: boolean;
}) {
  return (
    <div
      className={`whitespace-pre-wrap text-[0.875rem] leading-[1.5] text-text ${
        mine ? "text-right" : "text-left"
      }`}
    >
      {highlight ? highlightText(text, highlight) : text}
    </div>
  );
}
