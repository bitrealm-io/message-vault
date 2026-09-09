/**
 * The Android SMS backup sources share one form: a backup folder, media
 * options, and the owner's phone numbers so the exporter can tell sent from
 * received. SMS Backup+ adds the owner's email addresses, because its archive
 * is Gmail-backed and the sender of a sent message is an email account.
 */
export const SMS_BACKUP_RESTORE_SOURCE = "sms-backup-restore";
export const GO_SMS_PRO_SOURCE = "go-sms-pro";
export const SMS_BACKUP_PLUS_SOURCE = "sms-backup-plus";

const ANDROID_SMS_SOURCES: ReadonlySet<string> = new Set([
  SMS_BACKUP_RESTORE_SOURCE,
  GO_SMS_PRO_SOURCE,
  SMS_BACKUP_PLUS_SOURCE,
]);

/** True for a source whose form asks for the backup device's phone numbers. */
export function isAndroidSmsSource(source: string): boolean {
  return ANDROID_SMS_SOURCES.has(source);
}

/**
 * True for the one Android source whose times need the account's zone.
 *
 * An SMS Backup+ archive writes a wall clock and no offset, so the instant
 * cannot be recovered without knowing where the phone was.
 */
export function needsAccountTimeZone(source: string): boolean {
  return source === SMS_BACKUP_PLUS_SOURCE;
}

/** True for the one Android source that also needs the owner's email addresses. */
export function needsOwnerEmails(source: string): boolean {
  return source === SMS_BACKUP_PLUS_SOURCE;
}

/** Split a typed list of addresses on commas, semicolons and whitespace; drops blanks. */
export function splitEmails(raw: string): string[] {
  return raw
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** What the backup folder holds, per source, for the form's hint line. */
export function backupFolderHint(source: string): string {
  switch (source) {
    case SMS_BACKUP_PLUS_SOURCE:
      return "Point at a folder of .eml files archived from SMS Backup+ (Gmail or IMAP). This does not connect to a mail server.";
    case GO_SMS_PRO_SOURCE:
      return "Point at the folder holding the GO SMS Pro backup files.";
    default:
      return "Point at a folder of SMS Backup & Restore XML files (not a single ZIP). Unlock encrypted backups before selecting the folder.";
  }
}
