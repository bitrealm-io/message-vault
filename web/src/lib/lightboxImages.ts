import { shownMimeType } from "./attachmentPreview";
import type { Message, MessageAttachment } from "./types";

/**
 * The images the viewer walks with previous and next, from the loaded
 * messages, and where the clicked one sits among them. The clicked
 * attachment is found as the object it is, not by its SHA-256: two messages
 * can carry the same photo, and the viewer opens at the one clicked.
 */
export function lightboxImages(
  messages: Message[],
  clicked: MessageAttachment,
): { items: MessageAttachment[]; index: number } {
  const items = messages.flatMap((m) =>
    (m.attachments || []).filter((a) => a.sha256 && shownMimeType(a)?.startsWith("image/")),
  );
  const index = items.indexOf(clicked);
  if (items.length === 0) return { items: [clicked], index: 0 };
  return { items, index: index >= 0 ? index : 0 };
}
