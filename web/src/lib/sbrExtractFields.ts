import type { AttachmentMediaMode, ExtractConfig } from "./types";

/** Build the extract payload fields shared by iPhone and SMS Backup & Restore media options. */
export function mediaExtractFields(args: {
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
}): Pick<
  ExtractConfig,
  "attachment_media" | "media_max_resolution" | "media_max_fps" | "media_min_size"
> {
  return {
    attachment_media: args.attachmentMedia,
    media_max_resolution: args.maxResolution,
    media_max_fps: args.maxFps,
    media_min_size: `${args.minSizeMb.trim() || "20"}M`,
  };
}

/**
 * Build extract options for the Android SMS sources (owner phones required;
 * SMS Backup+ also carries the owner's email addresses).
 */
export function sbrExtractFields(args: {
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
  ownerPhones: string[];
  ownerEmails?: string[];
  timeZone?: string;
  obfuscate: boolean;
}): Pick<
  ExtractConfig,
  | "attachment_media"
  | "media_max_resolution"
  | "media_max_fps"
  | "media_min_size"
  | "owner_phones"
  | "owner_emails"
  | "time_zone"
  | "obfuscate"
> {
  return {
    ...mediaExtractFields(args),
    owner_phones: args.ownerPhones,
    ...(args.ownerEmails && args.ownerEmails.length > 0 ? { owner_emails: args.ownerEmails } : {}),
    ...(args.timeZone ? { time_zone: args.timeZone } : {}),
    obfuscate: args.obfuscate,
  };
}
