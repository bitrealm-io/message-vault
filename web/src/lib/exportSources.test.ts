import { describe, expect, it } from "vitest";
import { EXPORT_SOURCES } from "./exportSources";
import { IMESSAGE_SOURCE_ID } from "./imessageImport";
import { WHATSAPP_SOURCE_ID } from "./whatsappImport";

describe("EXPORT_SOURCES", () => {
  it("lists one Apple Messages row instead of separate iOS and macOS sources", () => {
    const ids = EXPORT_SOURCES.map((s) => s.id);
    expect(ids).toContain(IMESSAGE_SOURCE_ID);
    expect(ids).not.toContain("imessage-ios");
    expect(ids).not.toContain("imessage-macos");
    expect(ids).toContain(WHATSAPP_SOURCE_ID);
    expect(ids).not.toContain("whatsapp-android");
    expect(ids).not.toContain("whatsapp-ios");
    expect(EXPORT_SOURCES.find((s) => s.id === WHATSAPP_SOURCE_ID)?.label).toBe("WhatsApp");
    expect(ids).toContain("sms-backup-restore");
    expect(EXPORT_SOURCES.find((s) => s.id === IMESSAGE_SOURCE_ID)?.label).toBe("Apple Messages");
    expect(new Set(ids).size).toBe(ids.length);
  });
});
