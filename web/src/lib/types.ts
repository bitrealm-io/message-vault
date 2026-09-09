import type { components } from "./vaultApi.types";

type Schema = components["schemas"];

/*
 * Shapes the vault returns come from the generated types, so a field renamed
 * on the server is a build error here rather than an empty screen. Shapes
 * below that the vault never sends — desktop command arguments, progress
 * events, and the per-app extras on a message — stay hand-written.
 */

/** One participant in a conversation, as the conversation list returns them. */
export type Participant = Schema["Participant"];

/** One conversation in the browse list. */
export type Conversation = Schema["ConversationSummary"];

/** One participant on a message. */
export type MessageParticipant = Schema["Participant"];

/** The conversation a message belongs to, as a message carries it. */
export type MessageConversation = Schema["MessageConversation"];

/** One attachment on a message. */
export type MessageAttachment = Schema["Attachment"];

/** One tapback reaction on a message. */
export type MessageTapback = Schema["Tapback"];

/** One message, as `GET /v1/conversations/{id}/messages` returns it — the
 * same row shape the Export routes return, since one loader serves both. */
export type Message = Schema["Message"];

export type AttachmentMediaMode = "copy" | "convert" | "compress" | "skip";

export interface ExtractConfig {
  source: string;
  path: string;
  output_dir: string;
  backup_password?: string;
  attachment_media?: AttachmentMediaMode;
  media_max_resolution?: string;
  media_max_fps?: string;
  media_min_size?: string;
  obfuscate?: boolean;
  /** Owner phone numbers for Android SMS exporters (repeatable). */
  owner_phones?: string[];
  /** Owner email addresses for SMS Backup+ (repeatable). */
  owner_emails?: string[];
  /**
   * The account's IANA time zone, e.g. `America/New_York`. SMS Backup+ archive
   * transcripts carry a wall clock with no offset and are read in it.
   */
  time_zone?: string;
  /** Alternate folder for Attachments and StickerCache (Mac and jailbreak). */
  attachment_root?: string;
  /** Path to an Apple AddressBook file (Mac and jailbreak). */
  apple_contacts?: string;
  /** WhatsApp decryption key (file path or crypt15 hex). Not an Apple backup password. */
  whatsapp_key?: string;
  /** WhatsApp contacts database (`wa.db` or ContactsV2.sqlite). */
  whatsapp_wa?: string;
  /** WhatsApp media folder override. */
  whatsapp_media?: string;
  /** Explicit WhatsApp message database (`msgstore.db`). */
  whatsapp_db?: string;
  /** iPhone WhatsApp Business default files (`--business`). */
  whatsapp_business?: boolean;
  /** Continue an interrupted export in the same folder: previous output is
   * kept and conversations already written are skipped. */
  resume?: boolean;
}

export interface ExtractErrorEvent {
  detail: string;
  user_message?: string;
}

/**
 * One typed progress event from the desktop backend (`extract:progress`).
 * `setup` is a numbered step before any message is read (decrypting an
 * iPhone backup, caching chat tables); its label arrives as `status` and
 * `done`/`total` are the step's position, not message counts.
 */
export interface ImportProgressEvent {
  step: "setup" | "parse" | "attachments" | "prepare" | "media" | "upload";
  done: number;
  total: number;
  bytes_done?: number;
  bytes_total?: number;
  status?: string;
}

export interface ImportIssueEvent {
  kind: "error" | "skip";
  step: "parse" | "attachments" | "prepare" | "media" | "upload";
  item: string;
  reason: string;
}
