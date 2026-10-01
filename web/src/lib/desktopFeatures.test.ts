import { describe, expect, it } from "vitest";
import {
  canUseImportExport,
  canUseImportExportWithProfile,
  importExportBlock,
} from "./desktopFeatures.ts";

describe("canUseImportExport", () => {
  it("is true only in the desktop app", () => {
    expect(canUseImportExport(true)).toBe(true);
    expect(canUseImportExport(false)).toBe(false);
  });
});

describe("canUseImportExportWithProfile", () => {
  it("is false when the profile is missing even in the desktop app", () => {
    expect(canUseImportExportWithProfile(true, null)).toBe(false);
    expect(canUseImportExportWithProfile(true, undefined)).toBe(false);
  });

  it("is true only when a loaded profile is in the desktop app", () => {
    expect(canUseImportExportWithProfile(true, {})).toBe(true);
    expect(canUseImportExportWithProfile(false, {})).toBe(false);
  });
});

describe("importExportBlock", () => {
  const account = { can_import: true, can_export: true, is_demo: false };

  it("is null for a permission the account holds", () => {
    expect(importExportBlock("import", account)).toBeNull();
    expect(importExportBlock("export", account)).toBeNull();
  });

  it("reads each screen's own permission", () => {
    const importOff = { ...account, can_import: false };
    expect(importExportBlock("import", importOff)).toBe("not-allowed");
    expect(importExportBlock("export", importOff)).toBeNull();
    const exportOff = { ...account, can_export: false };
    expect(importExportBlock("export", exportOff)).toBe("not-allowed");
    expect(importExportBlock("import", exportOff)).toBeNull();
  });

  it("names the Demo Account on Import only", () => {
    const demo = { can_import: false, can_export: false, is_demo: true };
    expect(importExportBlock("import", demo)).toBe("demo");
    expect(importExportBlock("export", demo)).toBe("not-allowed");
  });
});
