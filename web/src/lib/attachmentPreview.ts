import type { MessageAttachment } from "./types";

/** Whether the server holds a preview of this attachment. */
export function hasPreview(attachment: MessageAttachment): boolean {
  return Boolean(attachment.sha256 && attachment.preview_mime_type);
}

/**
 * MIME type of what a conversation shows for an attachment: the preview's
 * when it has one, else the original's. The server says which attachments have
 * a preview, so nothing here lists the formats a browser can show.
 */
export function shownMimeType(attachment: MessageAttachment): string | null {
  return (hasPreview(attachment) ? attachment.preview_mime_type : attachment.mime_type) ?? null;
}
