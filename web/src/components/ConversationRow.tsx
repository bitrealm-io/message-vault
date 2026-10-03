import { type ReactNode, useId } from "react";
import { conversationName } from "../lib/conversationName";
import { formatDateSpan } from "../lib/formatDate";
import { conversationServiceLabel } from "../lib/serviceLabel";
import { useTimeZone } from "../lib/timeZone";
import { listRowDivider } from "../lib/tw";
import type { Conversation } from "../lib/types";
import Checkbox from "./Checkbox";
import { useColumnResizing } from "./columnResizeState";

function GroupIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className="inline-block shrink-0 align-[-1px]"
    >
      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
      <circle cx="7" cy="7" r="4" />
      <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );
}

/** Comma-separated names; each name stays whole; at most two lines then ellipsis. */
function GroupNames({ conv }: { conv: Conversation }) {
  return (
    <span className="line-clamp-2 break-words leading-[1.35]">
      {conv.participants.map((p, i) => (
        // contact_id and handle can both be absent for a participant with no
        // recorded address, so neither alone is safe as a key; name is never
        // empty (server rule), so the combination is always present, and
        // distinct unless two addressless, unlinked participants share a name.
        <span key={`${p.contact_id ?? ""}:${p.handle ?? ""}:${p.name}`}>
          {i > 0 ? ", " : null}
          <span className="whitespace-nowrap">{p.name}</span>
        </span>
      ))}
    </span>
  );
}

/**
 * The row title: an untitled group with people in it lists them, each kept
 * whole on a line; every other conversation shows {@link conversationName},
 * the name the Messages list gives it too.
 */
function titleContent(conv: Conversation): ReactNode {
  if (!conv.label && conv.is_group && conv.participants.length > 0) {
    return <GroupNames conv={conv} />;
  }
  return conversationTitleText(conv);
}

/** Plain-text form of the row title, for the checkbox's accessible name. */
function conversationTitleText(conv: Conversation): string {
  return conversationName({
    title: conv.label,
    isGroup: conv.is_group,
    participants: conv.participants,
  });
}

function GroupParticipantCount({ count }: { count: number }) {
  return (
    <span
      className="mt-[0.1rem] inline-flex shrink-0 items-center gap-1 text-[0.75rem] font-medium leading-[1.35] text-muted"
      title={`${count} participants`}
    >
      <span>{count}</span>
      <GroupIcon />
    </span>
  );
}

export default function ConversationRow({
  conversation,
  isSelected,
  onClick,
  checked,
  onCheckChange,
}: {
  conversation: Conversation;
  isSelected: boolean;
  onClick: () => void;
  checked?: boolean;
  onCheckChange?: (id: number) => void;
}) {
  const checkboxId = useId();
  const columnResizing = useColumnResizing();
  const isGroup = conversation.is_group;
  const wraps = isGroup && !conversation.label && !columnResizing;
  const dateSpan = formatDateSpan(
    conversation.date_range_start,
    conversation.last_message_at || conversation.date_range_end,
    useTimeZone(),
  );
  const bottomLeft = conversationServiceLabel(conversation);

  const body = (
    <div className="flex min-w-0 flex-1 flex-col gap-[0.3rem]">
      <div className="flex min-w-0 items-start justify-between gap-2">
        <span
          className={`min-w-0 flex-1 text-[0.875rem] font-medium leading-[1.35] text-text ${
            wraps ? "overflow-hidden" : "truncate"
          }`}
        >
          {titleContent(conversation)}
        </span>
        {isGroup ? <GroupParticipantCount count={conversation.participants.length} /> : null}
      </div>

      <div className="flex items-baseline justify-between gap-2 text-[0.75rem] text-muted">
        <span className="min-w-0 truncate">{bottomLeft}</span>
        {dateSpan ? <span className="shrink-0 text-right">{dateSpan}</span> : null}
      </div>
    </div>
  );

  const rowClass = `box-border flex w-full items-center gap-2 border-none px-[0.85rem] py-[0.7rem] text-left ${listRowDivider} ${
    isSelected ? "bg-hover" : "bg-transparent"
  }`;

  if (!onCheckChange) {
    return (
      <button type="button" onClick={onClick} className={`cursor-pointer ${rowClass}`}>
        {body}
      </button>
    );
  }

  // The checkbox is a sibling of the select-row button, never a child: a button
  // may not contain interactive content, and nesting it there left no way to
  // reach the checkbox by keyboard.
  return (
    <div className={rowClass}>
      {/*
        The hit area is the whole left gutter, not the 16px box: negative
        margins pull it out to the row's top, bottom, and left edges, and the
        padding puts it back so the box itself does not move. Anywhere left of
        the title toggles the row.
      */}
      <label
        htmlFor={checkboxId}
        className="-my-[0.7rem] -mr-2 -ml-[0.85rem] flex shrink-0 cursor-pointer items-center self-stretch pr-2 pl-[0.85rem]"
      >
        <Checkbox
          id={checkboxId}
          checked={checked || false}
          aria-label={`Select ${conversationTitleText(conversation)}`}
          onChange={() => onCheckChange(conversation.id)}
          className="shrink-0"
        />
      </label>
      <button
        type="button"
        onClick={onClick}
        className="flex min-w-0 flex-1 cursor-pointer items-start border-none bg-transparent p-0 text-left"
      >
        {body}
      </button>
    </div>
  );
}
