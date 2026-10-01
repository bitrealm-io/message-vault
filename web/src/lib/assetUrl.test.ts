import { describe, expect, it } from "vitest";
import { buildAssetPath, buildAssetPreviewPath } from "./assetUrl.ts";

describe("buildAssetPath", () => {
  it("includes sha and required source query", () => {
    expect(buildAssetPath("abc123", "imessage")).toBe("/v1/assets/abc123?source=imessage");
  });

  it("encodes special characters", () => {
    expect(buildAssetPath("deadbeef", "sms backup")).toBe(
      "/v1/assets/deadbeef?source=sms%20backup",
    );
  });

  it("rejects empty sha or source", () => {
    expect(() => buildAssetPath("", "imessage")).toThrow();
    expect(() => buildAssetPath("abc", "")).toThrow();
  });
});

describe("buildAssetPreviewPath", () => {
  it("names the preview under the original's sha", () => {
    expect(buildAssetPreviewPath("abc123", "sms backup")).toBe(
      "/v1/assets/abc123/preview?source=sms%20backup",
    );
  });

  it("rejects empty sha or source", () => {
    expect(() => buildAssetPreviewPath("", "imessage")).toThrow();
    expect(() => buildAssetPreviewPath("abc", "")).toThrow();
  });
});
