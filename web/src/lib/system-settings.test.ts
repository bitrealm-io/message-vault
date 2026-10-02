import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  defaultStagingDir,
  getImporterExtraPaths,
  getImporterPath,
  isUsableStagingParent,
  joinStagingPath,
  loadRememberedImportPaths,
  resolveImportStagingDir,
  setImporterExtraPath,
  setImporterPath,
  setRememberImporterPaths,
} from "./system-settings";

const mem = new Map<string, string>();

beforeEach(() => {
  mem.clear();
  const store: Storage = {
    getItem: (k) => mem.get(k) ?? null,
    setItem: (k, v) => {
      mem.set(k, String(v));
    },
    removeItem: (k) => {
      mem.delete(k);
    },
    clear: () => mem.clear(),
    key: () => null,
    length: 0,
  };
  // The node environment has no window. lib/storage.ts reads window.localStorage.
  vi.stubGlobal("localStorage", store);
  vi.stubGlobal("window", { localStorage: store });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("defaultStagingDir", () => {
  it("joins message-crate under the home folder", () => {
    expect(defaultStagingDir("/home/mbeisser")).toBe("/home/mbeisser/message-crate");
  });

  it("strips a trailing slash on the home folder", () => {
    expect(defaultStagingDir("/home/mbeisser/")).toBe("/home/mbeisser/message-crate");
  });

  it("uses a relative message-crate path when home is empty", () => {
    expect(defaultStagingDir("")).toBe("message-crate");
  });

  it("joins message-crate under a Unix root home", () => {
    expect(defaultStagingDir("/")).toBe("/message-crate");
  });
});

describe("isUsableStagingParent", () => {
  it("accepts an absolute folder", () => {
    expect(isUsableStagingParent("/data/imports")).toBe(true);
  });

  it("rejects the filesystem root", () => {
    expect(isUsableStagingParent("/")).toBe(false);
    expect(isUsableStagingParent("///")).toBe(false);
  });

  it("rejects a relative folder", () => {
    expect(isUsableStagingParent("message-crate")).toBe(false);
    expect(isUsableStagingParent("")).toBe(false);
  });
});

describe("joinStagingPath", () => {
  const now = new Date(2026, 7, 24, 18, 5, 9);

  it("puts the staging folder name directly under the parent", () => {
    expect(joinStagingPath("/home/mbeisser/message-crate", "imessage-ios", now)).toBe(
      "/home/mbeisser/message-crate/staging-iphone-ios-260824-180509",
    );
  });

  it("does not nest another message-crate under a custom parent", () => {
    expect(joinStagingPath("/data/imports", "imessage-ios", now)).toBe(
      "/data/imports/staging-iphone-ios-260824-180509",
    );
  });

  it("keeps a Unix root when stripping trailing slashes", () => {
    expect(joinStagingPath("/", "imessage-ios", now)).toBe("/staging-iphone-ios-260824-180509");
  });

  it("keeps SMS Backup & Restore source ids in the folder name", () => {
    expect(joinStagingPath("/Users/sam/message-crate", "sms-backup-restore", now)).toBe(
      "/Users/sam/message-crate/staging-sms-backup-restore-260824-180509",
    );
  });

  it("strips a trailing slash on the parent folder", () => {
    expect(joinStagingPath("/home/mbeisser/message-crate/", "imessage-macos", now)).toBe(
      "/home/mbeisser/message-crate/staging-macos-260824-180509",
    );
  });

  it("strips a trailing backslash on a Windows parent folder", () => {
    expect(joinStagingPath("C:\\Users\\sam\\message-crate\\", "imessage-ios", now)).toBe(
      "C:\\Users\\sam\\message-crate/staging-iphone-ios-260824-180509",
    );
  });

  it("uses only the staging folder name when the parent is empty", () => {
    expect(joinStagingPath("", "imessage-ios", now)).toBe("staging-iphone-ios-260824-180509");
  });
});

describe("resolveImportStagingDir", () => {
  it("fails when the user home directory cannot be determined and no parent is saved", async () => {
    await expect(resolveImportStagingDir("/backup", "imessage-ios")).rejects.toThrow(
      /home directory/i,
    );
  });

  it("joins a saved custom parent without nesting message-crate", async () => {
    localStorage.setItem("mc-staging-dir", "/data/imports");
    await expect(resolveImportStagingDir("/backup", "imessage-ios")).resolves.toMatch(
      /^\/data\/imports\/staging-iphone-ios-\d{6}-\d{6}$/,
    );
  });

  it("ignores a saved filesystem root and fails without a home directory", async () => {
    localStorage.setItem("mc-staging-dir", "/");
    await expect(resolveImportStagingDir("/backup", "imessage-ios")).rejects.toThrow(
      /home directory/i,
    );
  });

  it("ignores a saved relative parent and fails without a home directory", async () => {
    localStorage.setItem("mc-staging-dir", "message-crate");
    await expect(resolveImportStagingDir("/backup", "imessage-ios")).rejects.toThrow(
      /home directory/i,
    );
  });
});

describe("joinStagingPath jailbreak slug", () => {
  const now = new Date(2026, 7, 24, 18, 5, 9);

  it("uses iphone-jailbreak in the staging folder name", () => {
    expect(joinStagingPath("/home/sam/message-crate", "imessage-jailbreak", now)).toBe(
      "/home/sam/message-crate/staging-iphone-jailbreak-260824-180509",
    );
  });
});

describe("remembered importer extra paths", () => {
  it("keeps a legacy backup path string for imessage-ios", () => {
    setImporterPath("imessage-ios", "/backups/old-iphone");
    expect(getImporterPath("imessage-ios")).toBe("/backups/old-iphone");
    expect(getImporterExtraPaths("imessage-ios")).toEqual({
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
  });

  it("stores attachment folder and Apple Contacts per method", () => {
    setImporterPath("imessage-macos", "/Users/sam/Library/Messages/chat.db");
    setImporterExtraPath("imessage-macos", "attachmentRoot", "/Users/sam/Library/Messages");
    setImporterExtraPath(
      "imessage-macos",
      "appleContacts",
      "/Users/sam/Library/Application Support/AddressBook/AddressBook-v22.abcddb",
    );
    setImporterPath("imessage-jailbreak", "/mnt/iphone/sms.db");
    setImporterExtraPath("imessage-jailbreak", "attachmentRoot", "/mnt/iphone/Library/SMS");

    expect(getImporterPath("imessage-macos")).toBe("/Users/sam/Library/Messages/chat.db");
    expect(getImporterExtraPaths("imessage-macos")).toEqual({
      attachmentRoot: "/Users/sam/Library/Messages",
      appleContacts: "/Users/sam/Library/Application Support/AddressBook/AddressBook-v22.abcddb",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
    expect(getImporterPath("imessage-jailbreak")).toBe("/mnt/iphone/sms.db");
    expect(getImporterExtraPaths("imessage-jailbreak").attachmentRoot).toBe(
      "/mnt/iphone/Library/SMS",
    );
    expect(getImporterExtraPaths("imessage-macos").attachmentRoot).not.toBe(
      getImporterExtraPaths("imessage-jailbreak").attachmentRoot,
    );
  });

  it("clears an extra path when set to blank", () => {
    setImporterExtraPath("imessage-macos", "attachmentRoot", "/tmp/root");
    setImporterExtraPath("imessage-macos", "attachmentRoot", "  ");
    expect(getImporterExtraPaths("imessage-macos").attachmentRoot).toBe("");
  });
});

describe("loadRememberedImportPaths", () => {
  it("loads empty paths when remembering is off, whatever storage holds", () => {
    setImporterPath("imessage-macos", "/Users/sam/Library/Messages/chat.db");
    setImporterPath("whatsapp-android", "/tmp/wa");
    setImporterExtraPath("imessage-macos", "attachmentRoot", "/Users/sam/Library/Messages");
    setRememberImporterPaths(false);

    expect(loadRememberedImportPaths("imessage-macos")).toEqual({
      backupPath: "",
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
    expect(loadRememberedImportPaths("whatsapp-android")).toEqual({
      backupPath: "",
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
  });

  it("loads per-method paths when remembering is on", () => {
    setRememberImporterPaths(true);
    setImporterPath("imessage-ios", "/backups/iphone");
    setImporterPath("imessage-macos", "/Users/sam/Library/Messages/chat.db");
    setImporterExtraPath("imessage-macos", "attachmentRoot", "/Users/sam/Library/Messages");

    expect(loadRememberedImportPaths("imessage-ios")).toEqual({
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
    expect(loadRememberedImportPaths("imessage-macos")).toEqual({
      backupPath: "/Users/sam/Library/Messages/chat.db",
      attachmentRoot: "/Users/sam/Library/Messages",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
  });

  it("restores WhatsApp folders by method id and whatsappWa without mixing appleContacts", () => {
    setRememberImporterPaths(true);
    setImporterPath("whatsapp-ios", "/backups/iphone");
    setImporterPath("whatsapp-android", "/backups/android");
    setImporterExtraPath("whatsapp-android", "whatsappWa", "/backups/android/wa.db");
    setImporterExtraPath("whatsapp-android", "whatsappMedia", "/backups/android/media");
    setImporterExtraPath("whatsapp-android", "whatsappDb", "/backups/android/msgstore.db");
    setImporterExtraPath(
      "imessage-macos",
      "appleContacts",
      "/Users/sam/Library/Application Support/AddressBook/AddressBook-v22.abcddb",
    );

    expect(loadRememberedImportPaths("whatsapp-ios")).toEqual({
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "",
      whatsappMedia: "",
      whatsappDb: "",
    });
    expect(loadRememberedImportPaths("whatsapp-android")).toEqual({
      backupPath: "/backups/android",
      attachmentRoot: "",
      appleContacts: "",
      whatsappWa: "/backups/android/wa.db",
      whatsappMedia: "/backups/android/media",
      whatsappDb: "/backups/android/msgstore.db",
    });
    expect(loadRememberedImportPaths("imessage-macos").appleContacts).toBe(
      "/Users/sam/Library/Application Support/AddressBook/AddressBook-v22.abcddb",
    );
    expect(loadRememberedImportPaths("whatsapp-android").appleContacts).toBe("");
    expect(loadRememberedImportPaths("imessage-macos").whatsappWa).toBe("");
  });
});
