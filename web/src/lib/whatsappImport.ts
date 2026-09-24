export const WHATSAPP_SOURCE_ID = "whatsapp";

export const WHATSAPP_DEFAULT_METHOD = "whatsapp-android";

export const WHATSAPP_METHODS = [
  { id: "whatsapp-android", label: "Android" },
  { id: "whatsapp-ios", label: "iPhone" },
] as const;

export type WhatsappMethodId = (typeof WHATSAPP_METHODS)[number]["id"];

const WHATSAPP_METHOD_IDS = new Set<string>(WHATSAPP_METHODS.map((m) => m.id));

export function isWhatsappMethod(source: string): source is WhatsappMethodId {
  return WHATSAPP_METHOD_IDS.has(source);
}

export function whatsappShowsKey(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsMedia(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsDb(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsBusiness(method: WhatsappMethodId): boolean {
  return method === "whatsapp-ios";
}

export function whatsappShowsContactsDb(_method: WhatsappMethodId): boolean {
  return true;
}

export const WHATSAPP_CRYPT_NAMES = [
  "msgstore.db.crypt12",
  "msgstore.db.crypt14",
  "msgstore.db.crypt15",
] as const;

export function whatsappCryptRequired(hasMsgstoreDb: boolean, cryptName: string | null): boolean {
  return !hasMsgstoreDb && cryptName !== null;
}

export type PathStat = {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
};

export type WhatsappPathStats = {
  backup: PathStat | null;
  contactsDb: PathStat | null;
  media: PathStat | null;
  db: PathStat | null;
  hasMsgstoreDb: boolean;
  cryptName: string | null;
};

export function emptyWhatsappPathStats(): WhatsappPathStats {
  return {
    backup: null,
    contactsDb: null,
    media: null,
    db: null,
    hasMsgstoreDb: false,
    cryptName: null,
  };
}

export const WHATSAPP_ERR_PATH_MISSING = "This path does not exist.";
export const WHATSAPP_ERR_FOLDER_IS_FILE = "Pick the backup folder.";
export const WHATSAPP_ERR_CRYPT_KEY = "Decryption key is required for an encrypted backup.";
export const WHATSAPP_ERR_MUST_BE_FILE = "This path must be a file.";
export const WHATSAPP_ERR_MUST_BE_FOLDER = "This path must be a folder.";
export const WHATSAPP_ERR_OWNER_PHONE = "Owner's WhatsApp number is required.";

/**
 * Where the account holder's number comes from. An Android crypt backup
 * carries no owner, so the form's number is required there. An iPhone
 * backup carries it in WhatsApp's preferences; the form's number is a
 * fallback for a backup without that key, so the field is optional.
 */
export function whatsappOwnerPhoneRequired(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

type WhatsappCanImportArgs = {
  method: WhatsappMethodId;
  backupPath: string;
  key: string;
  contactsDb: string;
  media: string;
  db: string;
  ownerPhone: string;
  stats: WhatsappPathStats;
};

type WhatsappImportErrorKey = "backupPath" | "key" | "contactsDb" | "media" | "db" | "ownerPhone";

function checkOptionalPath(
  path: string,
  stat: PathStat | null,
  errors: Partial<Record<WhatsappImportErrorKey, string>>,
  key: WhatsappImportErrorKey,
  kindError: string,
  expectDirectory: boolean,
): void {
  const trimmed = path.trim();
  if (trimmed === "") {
    return;
  }
  if (stat === null) {
    return;
  }
  if (!stat.exists) {
    errors[key] = WHATSAPP_ERR_PATH_MISSING;
    return;
  }
  if (expectDirectory) {
    if (stat.isFile) {
      errors[key] = kindError;
    }
  } else if (stat.isDirectory) {
    errors[key] = kindError;
  }
}

export function whatsappCanImport(args: WhatsappCanImportArgs): {
  enabled: boolean;
  errors: Partial<Record<WhatsappImportErrorKey, string>>;
} {
  const errors: Partial<Record<WhatsappImportErrorKey, string>> = {};

  const backupPath = args.backupPath.trim();
  if (backupPath === "") {
    return { enabled: false, errors: {} };
  }

  if (args.stats.backup === null) {
    return { enabled: false, errors: {} };
  }

  const backupStat = args.stats.backup;
  if (!backupStat.exists) {
    errors.backupPath = WHATSAPP_ERR_PATH_MISSING;
  } else if (backupStat.isFile) {
    errors.backupPath = WHATSAPP_ERR_FOLDER_IS_FILE;
  }

  if (
    args.method === "whatsapp-android" &&
    whatsappCryptRequired(args.stats.hasMsgstoreDb, args.stats.cryptName) &&
    args.key.trim() === ""
  ) {
    errors.key = WHATSAPP_ERR_CRYPT_KEY;
  }

  if (whatsappOwnerPhoneRequired(args.method) && args.ownerPhone.trim() === "") {
    errors.ownerPhone = WHATSAPP_ERR_OWNER_PHONE;
  }

  if (whatsappShowsContactsDb(args.method)) {
    checkOptionalPath(
      args.contactsDb,
      args.stats.contactsDb,
      errors,
      "contactsDb",
      WHATSAPP_ERR_MUST_BE_FILE,
      false,
    );
  }

  if (whatsappShowsMedia(args.method)) {
    checkOptionalPath(
      args.media,
      args.stats.media,
      errors,
      "media",
      WHATSAPP_ERR_MUST_BE_FOLDER,
      true,
    );
  }

  if (whatsappShowsDb(args.method)) {
    checkOptionalPath(args.db, args.stats.db, errors, "db", WHATSAPP_ERR_MUST_BE_FILE, false);
  }

  const contactsCheckPending =
    whatsappShowsContactsDb(args.method) &&
    args.contactsDb.trim() !== "" &&
    args.stats.contactsDb === null;
  const mediaCheckPending =
    whatsappShowsMedia(args.method) && args.media.trim() !== "" && args.stats.media === null;
  const dbCheckPending =
    whatsappShowsDb(args.method) && args.db.trim() !== "" && args.stats.db === null;

  const enabled =
    Object.keys(errors).length === 0 &&
    backupPath !== "" &&
    !contactsCheckPending &&
    !mediaCheckPending &&
    !dbCheckPending;

  return { enabled, errors };
}
