import { mediaExtractFields } from "./sbrExtractFields";
import type { AttachmentMediaMode, ExtractConfig } from "./types";
import type { WhatsappMethodId } from "./whatsappImport";

/**
 * Build extract payload fields for a WhatsApp method.
 *
 * Media options apply to both platforms. The Android key, media folder,
 * and message database are sent only for Android when the trimmed value
 * is non-empty. Contacts database is sent on both platforms when
 * non-empty. WhatsApp Business is sent only for iPhone when the checkbox
 * is on. The owner's number goes on both platforms when non-empty, as the
 * one-entry `owner_phones` the desktop already reads for Android SMS:
 * Android's only source, iPhone's fallback when the backup carries no owner
 * key. Never sets `backup_password`.
 */
export function whatsappExtractFields(args: {
  source: WhatsappMethodId;
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
  key: string;
  wa: string;
  media: string;
  db: string;
  business: boolean;
  ownerPhone: string;
}): Pick<
  ExtractConfig,
  | "attachment_media"
  | "media_max_resolution"
  | "media_max_fps"
  | "media_min_size"
  | "whatsapp_key"
  | "whatsapp_wa"
  | "whatsapp_media"
  | "whatsapp_db"
  | "whatsapp_business"
  | "owner_phones"
> {
  const fields: ReturnType<typeof whatsappExtractFields> = {
    ...mediaExtractFields(args),
  };

  const ownerPhone = args.ownerPhone.trim();
  if (ownerPhone) {
    fields.owner_phones = [ownerPhone];
  }

  if (args.source === "whatsapp-android") {
    const key = args.key.trim();
    if (key) {
      fields.whatsapp_key = key;
    }
    const media = args.media.trim();
    if (media) {
      fields.whatsapp_media = media;
    }
    const db = args.db.trim();
    if (db) {
      fields.whatsapp_db = db;
    }
  }

  const wa = args.wa.trim();
  if (wa) {
    fields.whatsapp_wa = wa;
  }

  if (args.source === "whatsapp-ios" && args.business) {
    fields.whatsapp_business = true;
  }

  return fields;
}
