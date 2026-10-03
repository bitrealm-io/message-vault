import { describe, expect, it } from "vitest";
import { missingAttachmentChipLabel } from "./missingAttachmentLabel.ts";
import type { MessageAttachment } from "./types.ts";

function att(partial: Partial<MessageAttachment>): MessageAttachment {
  return {
    path: null,
    original_name: null,
    mime_type: null,
    sha256: null,
    is_sticker: false,
    transcription: null,
    missing_reason: null,
    ...partial,
  };
}

describe("missingAttachmentChipLabel", () => {
  it("formats too_large with name and mime", () => {
    expect(
      missingAttachmentChipLabel(
        att({
          original_name: "video.mp4",
          mime_type: "video/mp4",
          missing_reason: "too_large",
        }),
      ),
    ).toBe("video.mp4 · video/mp4 (missing — too large)");
  });

  it("formats file_missing from path basename when name missing", () => {
    expect(
      missingAttachmentChipLabel(
        att({
          path: "attachments/gone.bin",
          missing_reason: "file_missing",
        }),
      ),
    ).toBe("gone.bin (missing — file not found)");
  });

  it("labels a deliberately skipped attachment as skipped, not missing", () => {
    expect(
      missingAttachmentChipLabel(
        att({
          original_name: "IMG_0421.HEIC",
          mime_type: "image/heic",
          missing_reason: "not_copied",
        }),
      ),
    ).toBe("IMG_0421.HEIC · image/heic (skipped)");
  });

  it("keeps the ffmpeg detail from a convert_failed reason", () => {
    expect(
      missingAttachmentChipLabel(
        att({ original_name: "clip.mov", missing_reason: "convert_failed: no video stream" }),
      ),
    ).toBe("clip.mov (could not be converted — no video stream)");
  });

  it("shows an explicit unknown reason instead of swallowing it", () => {
    expect(
      missingAttachmentChipLabel(
        att({ original_name: "a.bin", missing_reason: "unknown: gremlins" }),
      ),
    ).toBe("a.bin (could not be imported — gremlins)");
  });

  it("keeps an unrecognized raw reason visible, worded like the unknown case", () => {
    expect(
      missingAttachmentChipLabel(att({ original_name: "a.bin", missing_reason: "weird_reason" })),
    ).toBe("a.bin (could not be imported — weird_reason)");
  });

  it("drops the trailing dash when a convert_failed reason has no detail", () => {
    expect(
      missingAttachmentChipLabel(
        att({ original_name: "clip.mov", missing_reason: "convert_failed: " }),
      ),
    ).toBe("clip.mov (could not be converted)");
  });

  it("drops the trailing dash when an unknown reason has no detail", () => {
    expect(
      missingAttachmentChipLabel(att({ original_name: "a.bin", missing_reason: "unknown: " })),
    ).toBe("a.bin (could not be imported)");
  });

  it("treats no_path and null as plain missing", () => {
    expect(
      missingAttachmentChipLabel(att({ original_name: "a.bin", missing_reason: "no_path" })),
    ).toBe("a.bin (missing)");
    expect(missingAttachmentChipLabel(att({ original_name: "a.bin" }))).toBe("a.bin (missing)");
  });
});
