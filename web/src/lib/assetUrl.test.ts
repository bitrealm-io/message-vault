import { describe, expect, it } from "vitest";
import { buildAssetPath, buildAssetPreviewPath } from "./assetUrl.ts";

describe("buildAssetPath", () => {
  it("names the asset by its sha alone", () => {
    expect(buildAssetPath("abc123")).toBe("/v1/assets/abc123");
  });

  it("encodes special characters", () => {
    expect(buildAssetPath("dead beef")).toBe("/v1/assets/dead%20beef");
  });

  it("rejects an empty sha", () => {
    expect(() => buildAssetPath("")).toThrow();
    expect(() => buildAssetPath("  ")).toThrow();
  });
});

describe("buildAssetPreviewPath", () => {
  it("names the preview under the original's sha", () => {
    expect(buildAssetPreviewPath("abc123")).toBe("/v1/assets/abc123/preview");
  });

  it("rejects an empty sha", () => {
    expect(() => buildAssetPreviewPath("")).toThrow();
  });
});
