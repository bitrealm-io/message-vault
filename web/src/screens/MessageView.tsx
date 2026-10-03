import { useCallback, useDeferredValue, useMemo, useState } from "react";
import AttachmentLightbox from "../components/AttachmentLightbox";
import {
  type ContactPreview,
  contactPreviewFromThreadParticipants,
} from "../components/contactDrawer/contactDrawerTypes";
import SourcesPanel from "../components/SourcesPanel";
import { lightboxImages } from "../lib/lightboxImages";
import { useTimeZone } from "../lib/timeZone";
import type { Conversation, MessageAttachment } from "../lib/types";
import ConversationHeader from "./message/ConversationHeader";
import MessageFindBar from "./message/MessageFindBar";
import MessageThread from "./message/MessageThread";
import { conversationYears, useConversationMessages } from "./message/useConversationMessages";

/**
 * The conversation panel, the right-hand side of the Messages screen, shown
 * the way a phone shows a conversation (#1391). `openAt` opens it at one
 * message, highlighted, with the messages around it, for a search result
 * (#313); without it, the panel opens at the newest message.
 */
export default function MessageView({
  conversation,
  openAt = null,
  onOpenContact,
}: {
  conversation: Conversation;
  openAt?: number | null;
  onOpenContact?: (contactId: string, preview: ContactPreview | null) => void;
}) {
  const thread = useConversationMessages(conversation.id, openAt);
  const { messages, find } = thread;

  const [lightboxItems, setLightboxItems] = useState<MessageAttachment[] | null>(null);
  const [lightboxIndex, setLightboxIndex] = useState(0);
  const [showSources, setShowSources] = useState(false);

  const zone = useTimeZone();
  const years = useMemo(
    () => conversationYears(conversation.date_range_start, conversation.date_range_end, zone),
    [conversation.date_range_start, conversation.date_range_end, zone],
  );

  // Open the image viewer at the clicked photo. Previous/next walks the loaded messages' images.
  const handleAttachmentClick = useCallback(
    (att: MessageAttachment) => {
      const { items, index } = lightboxImages(messages, att);
      setLightboxItems(items);
      setLightboxIndex(index);
    },
    [messages],
  );

  /** Prefer list-API participants; fall back to the loaded page's conversation header. */
  const participants =
    conversation.participants.length > 0
      ? conversation.participants
      : (messages[0]?.conversation.participants ?? []);
  const displayParticipants = useMemo(
    () =>
      participants.map((p) => ({
        label: p.name,
        contact_id: p.contact_id == null ? null : String(p.contact_id),
      })),
    [participants],
  );

  // Re-highlighting a whole thread is far more work than echoing a keystroke, so
  // the find bar stays on the term while the thread trails on the deferred one.
  const deferredFindTerm = useDeferredValue(find.open ? find.term : "");

  return (
    <div className="flex h-full flex-col">
      <ConversationHeader
        conversation={conversation}
        displayParticipants={displayParticipants}
        years={years}
        findOpen={find.open}
        onToggleFind={() => (find.open ? find.close() : find.openFind())}
        onJumpToNewest={thread.jumpToNewest}
        onJumpToYear={(year) => void thread.jumpToYear(year)}
        onOpenContact={(contactId) => {
          onOpenContact?.(contactId, contactPreviewFromThreadParticipants(contactId, participants));
        }}
        onShowSources={() => setShowSources(true)}
      />

      {find.open ? (
        <MessageFindBar
          findTerm={find.term}
          onFindTermChange={find.setTerm}
          matchCount={find.total}
          matchPosition={find.position}
          searching={find.searching}
          onPrevMatch={find.prevMatch}
          onNextMatch={find.nextMatch}
          onClose={find.close}
        />
      ) : null}

      <MessageThread
        messages={messages}
        loading={thread.loading}
        error={thread.error}
        findTerm={deferredFindTerm}
        highlightId={thread.highlightId}
        isGroup={conversation.is_group}
        hasOlder={thread.hasOlder}
        hasNewer={thread.hasNewer}
        loadingOlder={thread.loadingOlder}
        loadingNewer={thread.loadingNewer}
        onLoadOlder={thread.loadOlder}
        onLoadNewer={thread.loadNewer}
        landing={thread.landing}
        jumping={thread.jumping}
        onAttachmentClick={handleAttachmentClick}
      />

      {lightboxItems && (
        <AttachmentLightbox
          items={lightboxItems}
          currentIndex={lightboxIndex}
          onClose={() => setLightboxItems(null)}
          onPrev={() =>
            setLightboxIndex((i) => (i - 1 + lightboxItems.length) % lightboxItems.length)
          }
          onNext={() => setLightboxIndex((i) => (i + 1) % lightboxItems.length)}
        />
      )}

      {showSources && (
        <SourcesPanel conversationId={conversation.id} onClose={() => setShowSources(false)} />
      )}
    </div>
  );
}
