import { describe, expect, it } from "vitest";
import { sbrExtractFields } from "./sbrExtractFields";

describe("sbrExtractFields", () => {
  it("includes attachment media and owner phones for SMS Backup & Restore", () => {
    expect(
      sbrExtractFields({
        attachmentMedia: "convert",
        maxResolution: "1080p",
        maxFps: "30",
        minSizeMb: "20",
        ownerPhones: ["+15551111", "+15552222"],
        obfuscate: true,
      }),
    ).toEqual({
      attachment_media: "convert",
      media_max_resolution: "1080p",
      media_max_fps: "30",
      media_min_size: "20M",
      owner_phones: ["+15551111", "+15552222"],
      obfuscate: true,
    });
  });

  it("carries the account time zone when there is one", () => {
    // SMS Backup+ archive transcripts are wall-clock with no offset, so the
    // exporter refuses to run without this.
    const fields = sbrExtractFields({
      attachmentMedia: "copy",
      maxResolution: "720p",
      maxFps: "30",
      minSizeMb: "20",
      ownerPhones: ["+15551111"],
      timeZone: "America/New_York",
      obfuscate: false,
    });
    expect(fields.time_zone).toBe("America/New_York");
  });

  it("omits the time zone rather than sending an empty one", () => {
    const fields = sbrExtractFields({
      attachmentMedia: "copy",
      maxResolution: "720p",
      maxFps: "30",
      minSizeMb: "20",
      ownerPhones: ["+15551111"],
      timeZone: "",
      obfuscate: false,
    });
    expect(fields).not.toHaveProperty("time_zone");
  });
});
