import { useState } from "react";
import { ToggleButton } from "react-aria-components";
import { useNavigate } from "react-router-dom";
import PlainButton from "../../components/PlainButton";
import PopupMenu, { type PopupMenuItem } from "../../components/PopupMenu";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { formatMonthYear } from "../../lib/formatDate";
import { conversationServiceLabel } from "../../lib/serviceLabel";
import { useTimeZone } from "../../lib/timeZone";
import { useTrashConversation } from "../../lib/trash";
import type { Conversation } from "../../lib/types";
import ContactGroupFromConversation from "./ContactGroupFromConversation";
import { useContactGroupMembers } from "./contactGroupMembers";

const TOOL_CLASS =
  "cursor-pointer rounded-md border border-border bg-panel px-2.5 py-[0.2rem] text-[0.813rem] text-text outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent";

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
      // A person may be named like a fixed row ("Sources"); the menu keys
      // rows by id, so a person's row has one no fixed row can share.
      id: `participant:${label}`,
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
  const [groupDialogOpen, setGroupDialogOpen] = useState(false);

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
        <div className="ml-auto flex gap-1.5">
          <ToggleButton
            isSelected={findOpen}
            onChange={onToggleFind}
            className={`${TOOL_CLASS} data-[selected]:border-accent`}
          >
            Find
          </ToggleButton>
          <PopupMenu
            trigger={
              <PlainButton className={`${TOOL_CLASS} aria-expanded:border-accent`}>
                Jump to ▾
              </PlainButton>
            }
            label="Jump to"
            items={jumpItems}
            className="max-h-[60vh] overflow-y-auto"
          />
          <PopupMenu
            trigger={
              <PlainButton
                aria-label="More for this conversation"
                className={`${TOOL_CLASS} aria-expanded:border-accent`}
              >
                ⋯
              </PlainButton>
            }
            label="More for this conversation"
            items={moreItems}
            className="max-h-[60vh] min-w-[12rem] overflow-y-auto"
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
