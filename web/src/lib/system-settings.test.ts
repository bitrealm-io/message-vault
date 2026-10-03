import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  getImporterExtraPaths,
  getImporterPath,
  isUsableStagingParent,
  loadRememberedImportPaths,
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
