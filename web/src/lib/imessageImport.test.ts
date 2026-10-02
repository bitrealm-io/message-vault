import { describe, expect, it } from "vitest";
import {
  IMESSAGE_ERR_ATTACHMENT_IS_FILE,
  IMESSAGE_ERR_CONTACTS_IS_DIR,
  IMESSAGE_ERR_ENCRYPTED_PASSWORD,
  IMESSAGE_ERR_IPHONE_PATH_IS_FILE,
  IMESSAGE_ERR_JAILBREAK_PATH_IS_DIR,
  IMESSAGE_ERR_MAC_PATH_IS_DIR,
  IMESSAGE_ERR_PATH_MISSING,
  IMESSAGE_METHODS,
  imessageApplePlatform,
  imessageAttachmentRootRequired,
  imessageCanImport,
  imessageShowsAppleContacts,
  imessageShowsAttachmentRoot,
  imessageShowsPassword,
  imessageStatsForMethod,
  imessageVisiblePlatforms,
  isImessageMethod,
  macMessagesDbPath,
  shouldPrefillMacMessagesDb,
} from "./imessageImport";

const presentFile: { exists: true; isFile: true; isDirectory: false } = {
  exists: true,
  isFile: true,
  isDirectory: false,
};
const presentDir: { exists: true; isFile: false; isDirectory: true } = {
  exists: true,
  isFile: false,
  isDirectory: true,
};
const missing: { exists: false; isFile: false; isDirectory: false } = {
  exists: false,
  isFile: false,
  isDirectory: false,
};

describe("iMessage methods", () => {
  it("lists three methods", () => {
    expect(IMESSAGE_METHODS.map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
      "imessage-jailbreak",
    ]);
    expect(IMESSAGE_METHODS.map((m) => m.label)).toEqual([
      "Mac Messages",
      "iPhone backup",
      "Jailbroken iPhone",
    ]);
  });

  it("derives converter platform from the method", () => {
    expect(imessageApplePlatform("imessage-ios")).toBe("iOS");
    expect(imessageApplePlatform("imessage-macos")).toBe("macOS");
    expect(imessageApplePlatform("imessage-jailbreak")).toBe("macOS");
  });

  it("shows password only for iPhone backup", () => {
    expect(imessageShowsPassword("imessage-ios")).toBe(true);
    expect(imessageShowsPassword("imessage-macos")).toBe(false);
    expect(imessageShowsPassword("imessage-jailbreak")).toBe(false);
  });

  it("shows attachment root and Apple Contacts on Mac and jailbreak only", () => {
    expect(imessageShowsAttachmentRoot("imessage-macos")).toBe(true);
    expect(imessageShowsAttachmentRoot("imessage-jailbreak")).toBe(true);
    expect(imessageShowsAttachmentRoot("imessage-ios")).toBe(false);
    expect(imessageShowsAppleContacts("imessage-macos")).toBe(true);
    expect(imessageShowsAppleContacts("imessage-jailbreak")).toBe(true);
    expect(imessageShowsAppleContacts("imessage-ios")).toBe(false);
  });

  it("treats only the three method ids as iMessage methods", () => {
    expect(isImessageMethod("imessage-ios")).toBe(true);
    expect(isImessageMethod("whatsapp-android")).toBe(false);
    expect(isImessageMethod("imessage")).toBe(false);
  });

  it("hides jailbreak from the platform list unless it is already selected", () => {
    expect(imessageVisiblePlatforms("imessage-ios").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
    ]);
    expect(imessageVisiblePlatforms("imessage-macos").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
    ]);
    expect(imessageVisiblePlatforms("imessage-jailbreak").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
      "imessage-jailbreak",
    ]);
  });
});

describe("imessageCanImport", () => {
  it("enables iPhone backup when the folder exists", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  it("keeps password optional when encryption is unknown", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("requires password when Manifest.plist is marked encrypted", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPassword).toBe(IMESSAGE_ERR_ENCRYPTED_PASSWORD);
  });

  it("enables an encrypted backup when the password is filled", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "secret",
      stats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("rejects an iPhone backup path that is a .db file", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/copy/sms.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(IMESSAGE_ERR_IPHONE_PATH_IS_FILE);
    // The platform list hides the jailbreak method, so the error must not
    // send the person to it.
    expect(result.errors.backupPath).not.toMatch(/jailbr/i);
  });

  it("enables Mac Messages when chat.db exists", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/Users/sam/Library/Messages/chat.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("rejects Mac or jailbreak when the path is a directory", () => {
    const mac = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/Users/sam/Library/Messages",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(mac.enabled).toBe(false);
    expect(mac.errors.backupPath).toBe(IMESSAGE_ERR_MAC_PATH_IS_DIR);

    const jail = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/Library/SMS",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentDir,
        attachmentRoot: presentDir,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(jail.enabled).toBe(false);
    expect(jail.errors.backupPath).toBe(IMESSAGE_ERR_JAILBREAK_PATH_IS_DIR);
  });

  it("treats attachment root as required only for jailbreak", () => {
    expect(imessageAttachmentRootRequired("imessage-jailbreak")).toBe(true);
    expect(imessageAttachmentRootRequired("imessage-macos")).toBe(false);
    expect(imessageAttachmentRootRequired("imessage-ios")).toBe(false);
  });

  it("requires jailbreak sms.db and attachment folder", () => {
    const missingRoot = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(missingRoot.enabled).toBe(false);

    const ready = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: presentDir,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(ready.enabled).toBe(true);
  });

  it("disables Import when an optional extra path is set but missing", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "/tmp/missing-attachments",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: missing,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.attachmentRoot).toBe(IMESSAGE_ERR_PATH_MISSING);
  });

  it("rejects an attachment folder that is a file", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "/tmp/chat.db",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: presentFile,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.attachmentRoot).toBe(IMESSAGE_ERR_ATTACHMENT_IS_FILE);
  });

  it("rejects an Apple Contacts path that is a directory", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "",
      appleContacts: "/tmp/AddressBook",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: presentDir,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.appleContacts).toBe(IMESSAGE_ERR_CONTACTS_IS_DIR);
  });

  it("still validates Apple Contacts when jailbreak attachment root is empty", () => {
    const result = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "",
      appleContacts: "/tmp/AddressBook",
      backupPassword: "",
      stats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: presentDir,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.appleContacts).toBe(IMESSAGE_ERR_CONTACTS_IS_DIR);
  });

  it("disables Import while a non-empty path has not been checked yet", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: null,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
  });

  it("disables Import when the backup path is empty", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "  ",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: null,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
  });
});

describe("imessageStatsForMethod", () => {
  it("clears encryption state when leaving iPhone backup", () => {
    expect(
      imessageStatsForMethod("imessage-macos", {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      }),
    ).toEqual({
      backup: presentFile,
      attachmentRoot: null,
      appleContacts: null,
      backupEncrypted: null,
    });
  });
});

describe("Mac Messages pre-fill", () => {
  it("joins chat.db under the home Library folder", () => {
    expect(macMessagesDbPath("/Users/sam")).toBe("/Users/sam/Library/Messages/chat.db");
    expect(macMessagesDbPath("/Users/sam/")).toBe("/Users/sam/Library/Messages/chat.db");
  });

  it("pre-fills only on macOS when the file exists and nothing is remembered", () => {
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: true,
        rememberedPath: "",
      }),
    ).toBe("/Users/sam/Library/Messages/chat.db");
    expect(
      shouldPrefillMacMessagesDb({
        os: "linux",
        homeDir: "/home/sam",
        chatDbExists: true,
        rememberedPath: "",
      }),
    ).toBe("");
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: false,
        rememberedPath: "",
      }),
    ).toBe("");
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: true,
        rememberedPath: "/copied/chat.db",
      }),
    ).toBe("/copied/chat.db");
  });
});
