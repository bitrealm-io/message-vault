import type { MessageAttachment } from "./types";

/** File name shown on a missing-attachment chip. */
function attachmentDisplayName(attachment: MessageAttachment): string {
  if (attachment.original_name?.trim()) return attachment.original_name.trim();
  const path = attachment.path?.trim();
  if (path) {
    const parts = path.split(/[/\\]/);
    const base = parts[parts.length - 1];
    if (base) return base;
  }
  return "attachment";
}

/** Short reason shown in parentheses on a missing-attachment chip. */
function missingWhy(reason: string | null | undefined): string {
  if (!reason || reason === "no_path") return "missing";
  if (reason === "too_large") return "missing — too large";
  if (reason === "file_missing") return "missing — file not found";
  // Chosen on import ("Do not copy"), so the file is absent by request, not
  // lost.
  if (reason === "not_copied") return "skipped";
  if (reason.startsWith("convert_failed: ")) {
    const detail = reason.slice("convert_failed: ".length);
    return detail ? `could not be converted — ${detail}` : "could not be converted";
  }
  if (reason.startsWith("unknown: ")) {
    const detail = reason.slice("unknown: ".length);
    return detail ? `could not be imported — ${detail}` : "could not be imported";
  }
  // Keep an unrecognized reason visible and reportable, never uniform —
  // spec decision 41's fallback wording, whether or not it carries the
  // `unknown: ` prefix.
  return `could not be imported — ${reason}`;
}

/** Label for an attachment that was imported without the file bytes. */
export function missingAttachmentChipLabel(attachment: MessageAttachment): string {
  const name = attachmentDisplayName(attachment);
  const mime = attachment.mime_type?.trim();
  const why = missingWhy(attachment.missing_reason);
  if (mime) return `${name} · ${mime} (${why})`;
  return `${name} (${why})`;
}
