import { describe, expect, it } from "vitest";
import { whatsappExtractFields } from "./whatsappExtractFields";

describe("whatsappExtractFields", () => {
  it("sends Android key and omits empty wa, leftover business, and backup password", () => {
    expect(
      whatsappExtractFields({
        source: "whatsapp-android",
        attachmentMedia: "convert",
        maxResolution: "1080p",
        maxFps: "30",
        minSizeMb: "20",
        key: "deadbeef",
        wa: "  ",
        media: "/tmp/WhatsApp",
        db: "/tmp/msgstore.db",
        business: true,
        ownerPhone: " +1 555 555 0100 ",
      }),
    ).toEqual({
      attachment_media: "convert",
      media_max_resolution: "1080p",
      media_max_fps: "30",
      media_min_size: "20M",
      whatsapp_key: "deadbeef",
      whatsapp_media: "/tmp/WhatsApp",
      whatsapp_db: "/tmp/msgstore.db",
      owner_phones: ["+1 555 555 0100"],
    });
  });

  // The desktop reads the number as the one-entry owner phone list it already
  // takes for Android SMS; an empty field sends nothing, so iPhone falls back
  // to the number in the backup.
  it("sends the owner's number on iPhone too, and nothing when it is empty", () => {
    const base = {
      source: "whatsapp-ios" as const,
      attachmentMedia: "copy" as const,
      maxResolution: "720p",
      maxFps: "30",
      minSizeMb: "20",
      key: "",
      wa: "",
      media: "",
      db: "",
      business: false,
    };
    expect(whatsappExtractFields({ ...base, ownerPhone: "+15555550100" }).owner_phones).toEqual([
      "+15555550100",
    ]);
    expect(whatsappExtractFields({ ...base, ownerPhone: "  " }).owner_phones).toBeUndefined();
  });

  it("omits leftover Android media and db on iPhone", () => {
    expect(
      whatsappExtractFields({
        source: "whatsapp-ios",
        attachmentMedia: "copy",
        maxResolution: "720p",
        maxFps: "30",
        minSizeMb: "20",
        key: "",
        wa: "/backups/ContactsV2.sqlite",
        media: "/tmp/WhatsApp",
        db: "/tmp/msgstore.db",
        business: false,
        ownerPhone: "",
      }),
    ).toEqual({
      attachment_media: "copy",
      media_max_resolution: "720p",
      media_max_fps: "30",
      media_min_size: "20M",
      whatsapp_wa: "/backups/ContactsV2.sqlite",
    });
  });

  it("sets iPhone business and omits leftover key", () => {
    expect(
      whatsappExtractFields({
        source: "whatsapp-ios",
        attachmentMedia: "copy",
        maxResolution: "720p",
        maxFps: "30",
        minSizeMb: "20",
        key: "leftover",
        wa: "/backups/ContactsV2.sqlite",
        media: "",
        db: "",
        business: true,
        ownerPhone: "",
      }),
    ).toEqual({
      attachment_media: "copy",
      media_max_resolution: "720p",
      media_max_fps: "30",
      media_min_size: "20M",
      whatsapp_wa: "/backups/ContactsV2.sqlite",
      whatsapp_business: true,
    });
  });
});
