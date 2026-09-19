import { describe, expect, it } from "vitest";
import { APP_BUILD } from "./build";
import { formatBuild, productVersionOf, productVersionsDiffer } from "./buildFormat";

describe("formatBuild", () => {
  // The same cases are asserted in `crates/libs/build-version/src/lib.rs`.
  it("joins the version and its metadata", () => {
    expect(formatBuild("0.9.0", "343fe0d8")).toBe("0.9.0+343fe0d8");
    expect(formatBuild("0.9.0", "343fe0d8.dirty")).toBe("0.9.0+343fe0d8.dirty");
    expect(formatBuild("0.9.0", "unknown")).toBe("0.9.0+unknown");
  });

  it("writes a release as the version alone", () => {
    expect(formatBuild("0.9.0", "")).toBe("0.9.0");
  });
});

describe("productVersionOf", () => {
  it("reads the part before the commit", () => {
    expect(productVersionOf("0.9.0+343fe0d8.dirty")).toBe("0.9.0");
    expect(productVersionOf("0.9.0")).toBe("0.9.0");
  });
});

describe("productVersionsDiffer", () => {
  it("ignores the commit, so two dev builds of one release match", () => {
    expect(productVersionsDiffer("0.9.0+343fe0d8", "0.9.0+aaaa1111.dirty")).toBe(false);
    expect(productVersionsDiffer("0.9.0", "0.9.0+343fe0d8")).toBe(false);
  });

  it("flags a different release", () => {
    expect(productVersionsDiffer("0.10.0", "0.9.0+343fe0d8")).toBe(true);
  });
});

describe("APP_BUILD", () => {
  it("is embedded by Vite and starts with the Product Version", async () => {
    const pkg = await import("../../package.json");
    expect(productVersionOf(APP_BUILD)).toBe(pkg.version);
  });
});
