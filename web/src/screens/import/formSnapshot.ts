/**
 * The import form as the server stores it on a session, and the way back.
 *
 * `formSnapshot` writes the record at session creation; `restoreFormFromSnapshot`
 * rebuilds form values from it when Import reopens on that session. They sit
 * together so a field added to one is added to the other in the same place.
 */
import type { AttachmentMediaMode } from "../../lib/types";
import type { ImportJobFormValues } from "./useImportJob";

/** The one secret an Import Run can be started with: neither is ever stored. */
export type SnapshotSecret = "backupPassword" | "whatsappKey";

/**
 * Form snapshot for the session record, without the secrets.
 *
 * It carries `assetMaxBytes`, the server's attachment size limit as the run
 * read it before Staging, so a resumed run works to the same number.
 *
 * It records that a backup password or WhatsApp key was given, never the
 * secret itself, so a resume that reads the backup again knows to ask for
 * it. Each is recorded only for the source whose extract reads it: the
 * iPhone backup password and the Android WhatsApp key.
 */
export function formSnapshot(form: ImportJobFormValues): Record<string, unknown> {
  const { backupPassword, whatsappKey, ...rest } = form;
  return {
    ...rest,
    backupPasswordGiven: form.source === "imessage-ios" && backupPassword.trim() !== "",
    whatsappKeyGiven: form.source === "whatsapp-android" && whatsappKey.trim() !== "",
  };
}

/**
 * Which secret the stored Import Run was started with, or null for none.
 *
 * Read from the raw snapshot, because the rebuilt form values carry only
 * the secrets themselves, and those come back empty.
 */
export function snapshotSecret(raw: unknown): SnapshotSecret | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (r.backupPasswordGiven === true) return "backupPassword";
  if (r.whatsappKeyGiven === true) return "whatsappKey";
  return null;
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
 * The snapshot omits `backupPassword` and `whatsappKey`, so both come back
 * as "". A resume into Upload, a Review, or Media reads no backup and needs
 * neither. A resume during Staging and a restart run extract again: when
 * `snapshotSecret` says the run had one, the Resume Import panel asks for
 * it and the screen fills it in before the import starts.
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
  if (typeof r.obfuscate !== "boolean") return null;
  if (typeof r.timeZone !== "string") return null;
  if (typeof r.isAndroidSms !== "boolean") return null;
  if (typeof r.attachmentRoot !== "string") return null;
  if (typeof r.appleContacts !== "string") return null;
  if (typeof r.whatsappWa !== "string") return null;
  if (typeof r.whatsappMedia !== "string") return null;
  if (typeof r.whatsappDb !== "string") return null;
  if (typeof r.whatsappBusiness !== "boolean") return null;
  if (typeof r.whatsappOwnerPhone !== "string") return null;
  if (typeof r.backupPasswordGiven !== "boolean") return null;
  if (typeof r.whatsappKeyGiven !== "boolean") return null;
  // The limit the run was created under. Every stage after creation measures
  // against it, so a run without one cannot be picked up.
  if (typeof r.assetMaxBytes !== "number" || !(r.assetMaxBytes > 0)) return null;

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
    obfuscate: r.obfuscate,
    timeZone: r.timeZone,
    isAndroidSms: r.isAndroidSms,
    attachmentRoot: r.attachmentRoot,
    appleContacts: r.appleContacts,
    whatsappKey: "",
    whatsappWa: r.whatsappWa,
    whatsappMedia: r.whatsappMedia,
    whatsappDb: r.whatsappDb,
    whatsappBusiness: r.whatsappBusiness,
    whatsappOwnerPhone: r.whatsappOwnerPhone,
    assetMaxBytes: r.assetMaxBytes,
  };
}
