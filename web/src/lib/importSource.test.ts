import { describe, expect, it } from "vitest";
import { EXPORT_SOURCES } from "./exportSources";
import { IMESSAGE_METHODS, IMESSAGE_SOURCE_ID } from "./imessageImport";
import { importSessionCreateBody, sourceForMethod } from "./importSource";
import { WHATSAPP_METHODS, WHATSAPP_SOURCE_ID } from "./whatsappImport";

describe("sourceForMethod", () => {
  it("maps each iMessage method id to imessage", () => {
    expect(IMESSAGE_METHODS.map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
      "imessage-jailbreak",
    ]);
    for (const method of IMESSAGE_METHODS) {
      expect(sourceForMethod(method.id)).toBe(IMESSAGE_SOURCE_ID);
    }
  });

  it("maps each WhatsApp method id to whatsapp", () => {
    expect(WHATSAPP_METHODS.map((m) => m.id)).toEqual(["whatsapp-android", "whatsapp-ios"]);
    for (const method of WHATSAPP_METHODS) {
      expect(sourceForMethod(method.id)).toBe(WHATSAPP_SOURCE_ID);
    }
  });

  it("leaves each picker source id unchanged", () => {
    for (const source of EXPORT_SOURCES) {
      expect(sourceForMethod(source.id)).toBe(source.id);
    }
  });

  it("leaves sms-backup-restore unchanged", () => {
    expect(sourceForMethod("sms-backup-restore")).toBe("sms-backup-restore");
  });

  it("returns an unknown string unchanged", () => {
    expect(sourceForMethod("not-a-real-source")).toBe("not-a-real-source");
  });
});

describe("importSessionCreateBody", () => {
  it("sends imessage when the form method is imessage-ios", () => {
    expect(importSessionCreateBody("imessage-ios")).toEqual({
      source: "imessage",
      tool: "message-crate",
      mode: "append",
    });
  });

  it("sends whatsapp when the form method is whatsapp-android", () => {
    expect(importSessionCreateBody("whatsapp-android")).toEqual({
      source: "whatsapp",
      tool: "message-crate",
      mode: "append",
    });
  });

  it("sends sms-backup-restore unchanged", () => {
    expect(importSessionCreateBody("sms-backup-restore").source).toBe("sms-backup-restore");
  });
});
