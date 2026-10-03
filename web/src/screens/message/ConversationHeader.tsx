import { useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import PopupMenu, { type PopupMenuItem } from "../../components/PopupMenu";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { formatMonthYear } from "../../lib/formatDate";
import { conversationServiceLabel } from "../../lib/serviceLabel";
import { useTimeZone } from "../../lib/timeZone";
import { useTrashConversation } from "../../lib/trash";
import type { Conversation } from "../../lib/types";
import { Z_POPOVER } from "../../lib/zLayers";
import ContactGroupFromConversation from "./ContactGroupFromConversation";
import { useContactGroupMembers } from "./contactGroupMembers";

const TOOL_CLASS =
  "cursor-pointer rounded-md border border-border bg-panel px-2.5 py-[0.2rem] text-[0.813rem] text-text hover:bg-hover";

/** The menu rows for the people in the conversation; one with a contact opens it. */
function participantItems(
  participants: { label: string; contact_id?: string | null }[],
  onOpenContact: ((contactId: string) => void) | undefined,
): PopupMenuItem[] {
  const seen = new Map<string, number>();
  return participants.map((p) => {
    // The menu keys its rows by label, and two people can share a name.
    const n = seen.get(p.label) ?? 0;
    seen.set(p.label, n + 1);
    const label = n === 0 ? p.label : `${p.label} (${n + 1})`;
    const contactId = p.contact_id;
    return {
      label,
      disabled: !contactId,
      onSelect: () => {
        if (contactId) onOpenContact?.(contactId);
      },
      children: (
        <span className="flex items-center gap-2">
          <span className={contactId ? "text-accent" : "text-muted"}>{p.label}</span>
        </span>
      ),
    };
  });
}

/**
 * The conversation panel's one-line header (#1391): the conversation's name,
 * how many people are in a group, the service the way the conversation list
 * names it, the date range and the message count; then Find, Jump to (Newest
 * and every year) and a ⋯ menu holding the people, Sources, Move to trash and
 * Make a Contact Group.
 */
export default function ConversationHeader({
  conversation,
  displayParticipants,
  years,
  findOpen,
  onToggleFind,
  onJumpToNewest,
  onJumpToYear,
  onOpenContact,
  onShowSources,
}: {
  conversation: Conversation;
  displayParticipants: { label: string; contact_id?: string | null }[];
  /** The years the conversation spans, oldest first. */
  years: number[];
  findOpen: boolean;
  onToggleFind: () => void;
  onJumpToNewest: () => void;
  onJumpToYear: (year: number) => void;
  onOpenContact?: (contactId: string) => void;
  onShowSources: () => void;
}) {
  const zone = useTimeZone();
  const navigate = useNavigate();
  const trashConversation = useTrashConversation();
  const groupMembers = useContactGroupMembers(conversation);
  const [jumpOpen, setJumpOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [groupDialogOpen, setGroupDialogOpen] = useState(false);
  const jumpRef = useRef<HTMLButtonElement>(null);
  const moreRef = useRef<HTMLButtonElement>(null);

  // The conversation just left the list this thread was opened from, so go
  // back to it rather than leave the person on a thread that has quietly gone.
  const handleMoveToTrash = () => {
    trashConversation.mutate(conversation.id, { onSuccess: () => navigate("/") });
  };

  const title =
    conversation.label ||
    (conversation.is_group
      ? `${conversation.participants.length} participants`
      : conversation.participants[0]?.name);
  const service = conversationServiceLabel(conversation);

  const jumpItems: PopupMenuItem[] = [
    { label: "Newest", onSelect: onJumpToNewest },
    ...[...years].reverse().map((year) => ({
      label: String(year),
      onSelect: () => onJumpToYear(year),
    })),
  ];

  const moreItems: PopupMenuItem[] = [
    ...participantItems(displayParticipants, onOpenContact),
    { label: "Sources", onSelect: onShowSources },
    {
      label: "Move to trash",
      onSelect: handleMoveToTrash,
      disabled: trashConversation.isPending,
      children: trashConversation.isPending ? "Moving to trash…" : "Move to trash",
    },
    ...(groupMembers.length > 0
      ? [{ label: "Make a Contact Group", onSelect: () => setGroupDialogOpen(true) }]
      : []),
  ];

  return (
    <div className="border-b border-border bg-elevated px-4 py-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h2 className="m-0 min-w-0 truncate text-[1rem] font-semibold text-text">{title}</h2>
        <div className="flex min-w-0 flex-wrap gap-x-2.5 text-[0.813rem] text-muted">
          {conversation.is_group ? <span>{conversation.participants.length} people</span> : null}
          {service ? <span>{service}</span> : null}
          {conversation.date_range_start && conversation.date_range_end ? (
            <span>
              {formatMonthYear(conversation.date_range_start, zone)} –{" "}
              {formatMonthYear(conversation.date_range_end, zone)}
            </span>
          ) : null}
          <span>{conversation.message_count.toLocaleString()} messages</span>
        </div>
        <div className="relative ml-auto flex gap-1.5">
          <button
            type="button"
            aria-pressed={findOpen}
            onClick={onToggleFind}
            className={`${TOOL_CLASS} ${findOpen ? "border-accent" : ""}`}
          >
            Find
          </button>
          <button
            type="button"
            ref={jumpRef}
            aria-haspopup="menu"
            aria-expanded={jumpOpen}
            onClick={() => setJumpOpen((o) => !o)}
            className={`${TOOL_CLASS} ${jumpOpen ? "border-accent" : ""}`}
          >
            Jump to ▾
          </button>
          <button
            type="button"
            ref={moreRef}
            aria-haspopup="menu"
            aria-expanded={moreOpen}
            aria-label="More for this conversation"
            onClick={() => setMoreOpen((o) => !o)}
            className={`${TOOL_CLASS} ${moreOpen ? "border-accent" : ""}`}
          >
            ⋯
          </button>
          <PopupMenu
            open={jumpOpen}
            onClose={() => setJumpOpen(false)}
            triggerRef={jumpRef}
            label="Jump to"
            items={jumpItems}
            className={`absolute top-full right-9 mt-1 max-h-[60vh] overflow-y-auto ${Z_POPOVER}`}
          />
          <PopupMenu
            open={moreOpen}
            onClose={() => setMoreOpen(false)}
            triggerRef={moreRef}
            label="More for this conversation"
            items={moreItems}
            className={`absolute top-full right-0 mt-1 max-h-[60vh] min-w-[12rem] overflow-y-auto ${Z_POPOVER}`}
          />
        </div>
      </div>

      <ContactGroupFromConversation
        conversation={conversation}
        open={groupDialogOpen}
        onClose={() => setGroupDialogOpen(false)}
      />

      {trashConversation.error && (
        <div className="mt-2 rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.75rem] text-danger">
          {apiErrorMessage(trashConversation.error, "Could not move this conversation to trash.")}
        </div>
      )}
    </div>
  );
}
