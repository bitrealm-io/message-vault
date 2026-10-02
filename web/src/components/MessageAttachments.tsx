import { shownMimeType } from "../lib/attachmentPreview";
import type { Message, MessageAttachment } from "../lib/types";
import AttachmentThumbnail from "./AttachmentThumbnail";
import VideoPlayer from "./VideoPlayer";

/** Shared attachment strip for every service bubble. */
export default function MessageAttachments({
  message,
  onAttachmentClick,
}: {
  message: Message;
  onAttachmentClick?: (attachment: MessageAttachment, source: string) => void;
}) {
  if (!message.attachments.length) return null;

  return (
    <div>
      {message.attachments.map((att, i) =>
        att.sha256 && !att.missing_reason && shownMimeType(att)?.startsWith("video/") ? (
          <VideoPlayer key={att.sha256 ?? att.path ?? i} attachment={att} source={message.source} />
        ) : (
          <AttachmentThumbnail
            key={att.sha256 ?? att.path ?? i}
            attachment={att}
            source={message.source}
            onClick={() => onAttachmentClick?.(att, message.source)}
          />
        ),
      )}
    </div>
  );
}
