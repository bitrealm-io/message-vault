/**
 * The import form as the vault stores it on a session, and the way back.
 *
 * `formSnapshot` writes the record at session creation; `restoreFormFromSnapshot`
 * rebuilds form values from it when Import reopens on that session. They sit
 * together so a field added to one is added to the other in the same place.
 */
import type { AttachmentMediaMode } from "../../lib/types";
import type { ImportJobFormValues } from "./useImportJob";

/** Form snapshot for the session record, without the secrets. */
export function formSnapshot(form: ImportJobFormValues): Record<string, unknown> {
  const { backupPassword: _backupPassword, whatsappKey: _whatsappKey, ...rest } = form;
  return rest;
}

const ATTACHMENT_MEDIA_MODES: readonly AttachmentMediaMode[] = [
  "copy",
  "convert",
  "compress",
  "skip",
];
/** True for an array whose every element is a string. */
export function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

/**
 * Rebuild form values from a session's stored snapshot.
 *
 * The snapshot omits `backupPassword` and `whatsappKey`, defaulted to ""
 * here: the resume path never re-runs extract, and the push only reads
 * `force` and `attachmentMedia`, both present in the snapshot.
 *
 * The snapshot came from the database, not from this session's own state,
 * so its shape is checked field by field rather than trusted. Returns
 * null for anything that doesn't match, instead of throwing.
 */
export function restoreFormFromSnapshot(raw: unknown): ImportJobFormValues | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.source !== "string") return null;
  if (typeof r.backupPath !== "string") return null;
  if (
    typeof r.attachmentMedia !== "string" ||
    !ATTACHMENT_MEDIA_MODES.includes(r.attachmentMedia as AttachmentMediaMode)
  ) {
    return null;
  }
  if (typeof r.maxResolution !== "string") return null;
  if (typeof r.maxFps !== "string") return null;
  if (typeof r.minSizeMb !== "string") return null;
  if (!isStringArray(r.ownerPhones)) return null;
  // Snapshots written before SMS Backup+ had an email field carry none.
  const ownerEmails = isStringArray(r.ownerEmails) ? r.ownerEmails : [];
  // Likewise the time zone. A stored snapshot is a live record in the vault, so
  // an older one must still restore rather than make the session unreadable;
  // the zone is re-read from the account profile anyway.
  const timeZone = typeof r.timeZone === "string" ? r.timeZone : "";
  if (typeof r.force !== "boolean") return null;
  if (typeof r.obfuscate !== "boolean") return null;
  if (typeof r.isAndroidSms !== "boolean") return null;
  if (typeof r.attachmentRoot !== "string") return null;
  if (typeof r.appleContacts !== "string") return null;
  if (typeof r.whatsappWa !== "string") return null;
  if (typeof r.whatsappMedia !== "string") return null;
  if (typeof r.whatsappDb !== "string") return null;
  if (typeof r.whatsappBusiness !== "boolean") return null;

  return {
    source: r.source,
    backupPath: r.backupPath,
    backupPassword: "",
    attachmentMedia: r.attachmentMedia as AttachmentMediaMode,
    maxResolution: r.maxResolution,
    maxFps: r.maxFps,
    minSizeMb: r.minSizeMb,
    ownerPhones: r.ownerPhones,
    ownerEmails,
    timeZone,
    force: r.force,
    obfuscate: r.obfuscate,
    isAndroidSms: r.isAndroidSms,
    attachmentRoot: r.attachmentRoot,
    appleContacts: r.appleContacts,
    whatsappKey: "",
    whatsappWa: r.whatsappWa,
    whatsappMedia: r.whatsappMedia,
    whatsappDb: r.whatsappDb,
    whatsappBusiness: r.whatsappBusiness,
  };
}
