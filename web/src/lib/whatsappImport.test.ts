import { describe, expect, it } from "vitest";
import {
  isWhatsappMethod,
  WHATSAPP_DEFAULT_METHOD,
  WHATSAPP_ERR_CRYPT_KEY,
  WHATSAPP_ERR_FOLDER_IS_FILE,
  WHATSAPP_ERR_MUST_BE_FILE,
  WHATSAPP_ERR_MUST_BE_FOLDER,
  WHATSAPP_ERR_OWNER_PHONE,
  WHATSAPP_ERR_PATH_MISSING,
  WHATSAPP_SOURCE_ID,
  whatsappCanImport,
  whatsappCryptRequired,
  whatsappShowsBusiness,
  whatsappShowsKey,
} from "./whatsappImport";

const dir = { exists: true, isFile: false, isDirectory: true };
const file = { exists: true, isFile: true, isDirectory: false };

describe("whatsappImport", () => {
  it("keeps method ids and defaults Android", () => {
    expect(WHATSAPP_SOURCE_ID).toBe("whatsapp");
    expect(WHATSAPP_DEFAULT_METHOD).toBe("whatsapp-android");
    expect(isWhatsappMethod("whatsapp-android")).toBe(true);
    expect(isWhatsappMethod("whatsapp-ios")).toBe(true);
    expect(isWhatsappMethod("whatsapp")).toBe(false);
    expect(whatsappShowsKey("whatsapp-android")).toBe(true);
    expect(whatsappShowsKey("whatsapp-ios")).toBe(false);
    expect(whatsappShowsBusiness("whatsapp-ios")).toBe(true);
    expect(whatsappShowsBusiness("whatsapp-android")).toBe(false);
  });

  it("uses the spec error catalog", () => {
    expect(WHATSAPP_ERR_PATH_MISSING).toBe("This path does not exist.");
  });

  it("disables Import when the backup folder does not exist", () => {
    const missing = { exists: false, isFile: false, isDirectory: false };
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/missing-wa",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: missing,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(WHATSAPP_ERR_PATH_MISSING);
  });

  it("rejects an optional contacts path that does not exist", () => {
    const missing = { exists: false, isFile: false, isDirectory: false };
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "/tmp/missing-wa.db",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: missing,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.contactsDb).toBe(WHATSAPP_ERR_PATH_MISSING);
  });

  it("requires a key only when a crypt file is used", () => {
    expect(whatsappCryptRequired(true, "msgstore.db.crypt15")).toBe(false);
    expect(whatsappCryptRequired(false, "msgstore.db.crypt15")).toBe(true);
    expect(whatsappCryptRequired(false, null)).toBe(false);
  });

  it("disables Import when the backup path is empty", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "  ",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: null,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors).toEqual({});
  });

  it("disables Import when the Android folder is a file", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/msgstore.db",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: file,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(WHATSAPP_ERR_FOLDER_IS_FILE);
  });

  it("requires the key when only a crypt file is present", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: "msgstore.db.crypt15",
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.key).toBe(WHATSAPP_ERR_CRYPT_KEY);
  });

  it("enables Android Import for a folder with msgstore.db and no key", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: "msgstore.db.crypt15",
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  it("rejects an optional contacts path that is a directory", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "/tmp/wa.db",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: dir,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.contactsDb).toBe(WHATSAPP_ERR_MUST_BE_FILE);
  });

  it("rejects an optional media path that is a file", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "",
      media: "/tmp/media.txt",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: null,
        media: file,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.media).toBe(WHATSAPP_ERR_MUST_BE_FOLDER);
  });

  it("enables iPhone Import for a backup folder with no key", () => {
    const result = whatsappCanImport({
      method: "whatsapp-ios",
      backupPath: "/backups/iphone",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: dir,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  // An Android crypt backup carries no owner number, so the form's number
  // is the only source; an iPhone backup carries it in WhatsApp's
  // preferences, so the field is a fallback and may stay empty.
  it("requires the owner's number on Android and not on iPhone", () => {
    const stats = {
      backup: dir,
      contactsDb: null,
      media: null,
      db: null,
      hasMsgstoreDb: true,
      cryptName: null,
    };
    const android = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "  ",
      stats,
    });
    expect(android.enabled).toBe(false);
    expect(android.errors.ownerPhone).toBe(WHATSAPP_ERR_OWNER_PHONE);

    const iphone = whatsappCanImport({
      method: "whatsapp-ios",
      backupPath: "/backups/iphone",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "",
      stats,
    });
    expect(iphone.enabled).toBe(true);
    expect(iphone.errors).toEqual({});
  });
});
