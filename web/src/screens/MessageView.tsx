import { useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import AttachmentLightbox, { type LightboxItem } from "../components/AttachmentLightbox";
import {
  type ContactPreview,
  contactPreviewFromThreadParticipants,
} from "../components/contactDrawer/contactDrawerTypes";
import SourcesPanel from "../components/SourcesPanel";
import { shownMimeType } from "../lib/attachmentPreview";
import { useTimeZone } from "../lib/timeZone";
import type { Conversation, MessageAttachment } from "../lib/types";
import ConversationHeader from "./message/ConversationHeader";
import MessageFindBar from "./message/MessageFindBar";
import MessageThread from "./message/MessageThread";
import {
  buildFooterLabel,
  conversationYears,
  displaySourceLabel,
  PAGE_SIZE,
  useConversationMessages,
} from "./message/useConversationMessages";

export default function MessageView({
  conversation,
  onOpenContact,
}: {
  conversation: Conversation;
  onOpenContact?: (contactId: string, preview: ContactPreview | null) => void;
}) {
  const {
    messages,
    total,
    offset,
    activeYear,
    findTerm,
    setFindTerm,
    finding,
    activeMatch,
    setActiveMatch,
    loading,
    error,
    fetchConversationPage,
    selectAllYears,
    selectYear,
  } = useConversationMessages(conversation.id);

  const [lightboxItems, setLightboxItems] = useState<LightboxItem[] | null>(null);
  // Fallback source label from the first loaded message until the header has a better one.
  const sourceLabel = messages[0] ? displaySourceLabel(messages[0].source) : "unknown";
  const [lightboxIndex, setLightboxIndex] = useState(0);
  const [showSources, setShowSources] = useState(false);
  const [participantsOpen, setParticipantsOpen] = useState(true);

  const zone = useTimeZone();
  const years = useMemo(
    () => conversationYears(conversation.date_range_start, conversation.date_range_end, zone),
    [conversation.date_range_start, conversation.date_range_end, zone],
  );

  // Open the image viewer at the clicked photo. Previous/next walks this page's images.
  const handleAttachmentClick = useCallback(
    (att: MessageAttachment, source: string) => {
      const images = messages.flatMap((m) =>
        (m.attachments || [])
          .filter((a) => a.sha256 && shownMimeType(a)?.startsWith("image/"))
          .map((a) => ({ attachment: a, source: m.source })),
      );
      const idx = images.findIndex(
        (item) => item.attachment.sha256 === att.sha256 && item.source === source,
      );
      setLightboxItems(images.length > 0 ? images : [{ attachment: att, source }]);
      setLightboxIndex(idx >= 0 ? idx : 0);
    },
    [messages],
  );

  useEffect(() => {
    void conversation.id;
    setParticipantsOpen(true);
  }, [conversation.id]);

  /** Prefer list-API participants; fall back to the loaded page's conversation header. */
  const displayParticipants = useMemo(() => {
    const source =
      conversation.participants.length > 0
        ? conversation.participants
        : messages[0]?.conversation.participants || [];
    return source.map((p) => ({
      label: p.name,
      contact_id: p.contact_id == null ? null : String(p.contact_id),
    }));
  }, [conversation.participants, messages]);

  // Re-highlighting a whole thread is far more work than echoing a keystroke, so
  // the find bar stays on `findTerm` while the thread trails on the deferred one.
  const deferredFindTerm = useDeferredValue(findTerm);

  // While finding, the server already narrowed the rows to the matches, so
  // every loaded message is one. Ids arrive as numbers; the DOM ids are strings.
  const matchIds = useMemo(
    () => (finding ? messages.map((m) => String(m.id)) : []),
    [messages, finding],
  );

  // Scroll the current find match into view.
  useEffect(() => {
    if (!matchIds.length) return;
    const el = document.getElementById(`msg-${matchIds[activeMatch]}`);
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [activeMatch, matchIds]);

  const footerLabel = buildFooterLabel(activeYear, total, offset, finding);

  return (
    <div className="flex h-full flex-col">
      <ConversationHeader
        conversation={conversation}
        displayParticipants={displayParticipants}
        participantsOpen={participantsOpen}
        onToggleParticipants={() => setParticipantsOpen((o) => !o)}
        sourceLabel={sourceLabel}
        years={years}
        activeYear={activeYear}
        onSelectAllYears={selectAllYears}
        onSelectYear={selectYear}
        onOpenContact={(contactId) => {
          const participants =
            conversation.participants.length > 0
              ? conversation.participants
              : (messages[0]?.conversation.participants ?? []);
          onOpenContact?.(contactId, contactPreviewFromThreadParticipants(contactId, participants));
        }}
        onShowSources={() => setShowSources(true)}
      />

      <MessageFindBar
        findTerm={findTerm}
        onFindTermChange={setFindTerm}
        matchCount={finding ? total : 0}
        matchPosition={offset + activeMatch}
        activeYear={activeYear}
        onPrevMatch={() => setActiveMatch((a) => (a - 1 + matchIds.length) % matchIds.length)}
        onNextMatch={() => setActiveMatch((a) => (a + 1) % matchIds.length)}
      />

      <MessageThread
        messages={messages}
        loading={loading}
        error={error}
        findTerm={deferredFindTerm}
        matchIds={matchIds}
        activeMatch={activeMatch}
        activeYear={activeYear}
        finding={finding}
        footerLabel={footerLabel}
        offset={offset}
        total={total}
        onPrevPage={() => void fetchConversationPage(Math.max(0, offset - PAGE_SIZE))}
        onNextPage={() => void fetchConversationPage(offset + PAGE_SIZE)}
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
