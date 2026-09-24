import { IMESSAGE_SOURCE_ID } from "./imessageImport";
import { WHATSAPP_SOURCE_ID } from "./whatsappImport";

/** The iMazing CSV export, whose dates carry no zone. */
export const IMAZING_SOURCE_ID = "imazing";

/** Backup sources offered by Import in the desktop app. */
export const EXPORT_SOURCES: { id: string; label: string }[] = [
  { id: IMESSAGE_SOURCE_ID, label: "iMessage" },
  { id: WHATSAPP_SOURCE_ID, label: "WhatsApp" },
  { id: "sms-backup-restore", label: "SMS Backup & Restore" },
  { id: "go-sms-pro", label: "GO SMS Pro" },
  { id: IMAZING_SOURCE_ID, label: "iMazing" },
  { id: "sms-backup-plus", label: "SMS Backup+" },
  { id: "openextract", label: "OpenExtract" },
];
