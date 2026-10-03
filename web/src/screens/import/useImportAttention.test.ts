import { describe, expect, it } from "vitest";
import { importAttentionFor } from "./useImportAttention";

describe("importAttentionFor", () => {
  it("says a run at a Review is waiting, in this window or on the server", () => {
    expect(importAttentionFor("staging_review", undefined, null)).toBe("waiting");
    expect(importAttentionFor("form", undefined, "media_review")).toBe("waiting");
  });

  it("says a paused Upload is paused, in this window or on the server", () => {
    // A failed Upload is paused (#1233), so without this the run that
    // failed would carry no badge at all.
    expect(importAttentionFor("done", "paused", "upload")).toBe("paused");
    expect(importAttentionFor("form", undefined, "upload")).toBe("paused");
  });

  it("says a run that ended by failing failed", () => {
    expect(importAttentionFor("done", "failed", null)).toBe("failed");
  });

  it("carries no badge while this window runs a stage, or when nothing needs the person", () => {
    expect(importAttentionFor("running", undefined, "upload")).toBeNull();
    expect(importAttentionFor("form", undefined, null)).toBeNull();
    expect(importAttentionFor("form", undefined, "write")).toBeNull();
    expect(importAttentionFor("done", "completed", null)).toBeNull();
  });
});
