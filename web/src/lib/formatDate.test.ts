import { describe, expect, it } from "vitest";
import {
  formatDateSpan,
  formatDay,
  formatIsoDateOnly,
  formatMonthYear,
  formatUnixDate,
} from "./formatDate.ts";

describe("formatDay", () => {
  it("includes year, month, and day", () => {
    const out = formatDay("2024-09-09T12:00:00.000Z", "UTC");
    expect(out).toMatch(/2024/);
    expect(out).toMatch(/9/);
  });

  it("reads the day in the account's zone, not the browser's", () => {
    // 04:59 UTC on New Year's Day is still New Year's Eve in New York.
    const instant = "2025-01-01T04:59:00Z";
    expect(formatDay(instant, "UTC")).toMatch(/2025/);
    expect(formatDay(instant, "America/New_York")).toMatch(/2024/);
  });
});

describe("formatMonthYear", () => {
  it("omits the day", () => {
    const out = formatMonthYear("2024-09-09T12:00:00.000Z", "UTC");
    expect(out).toMatch(/2024/);
    expect(out).not.toMatch(/^\d/);
  });

  it("reads the month in the account's zone", () => {
    expect(formatMonthYear("2024-10-01T02:00:00Z", "America/Los_Angeles")).toMatch(/Sep/);
  });
});

describe("formatDateSpan", () => {
  it("returns null when both ends are missing", () => {
    expect(formatDateSpan(null, null, "UTC")).toBeNull();
  });

  it("returns a single day when only one end is set", () => {
    expect(formatDateSpan("2024-09-09T12:00:00.000Z", null, "UTC")).toBe(
      formatDay("2024-09-09T12:00:00.000Z", "UTC"),
    );
    expect(formatDateSpan(null, "2024-09-10T12:00:00.000Z", "UTC")).toBe(
      formatDay("2024-09-10T12:00:00.000Z", "UTC"),
    );
  });

  it("collapses identical formatted ends", () => {
    const day = "2024-06-15T12:00:00.000Z";
    expect(formatDateSpan(day, day, "UTC")).toBe(formatDay(day, "UTC"));
  });

  it("joins distinct formatted ends with an en dash", () => {
    const start = "2024-01-01T12:00:00.000Z";
    const end = "2024-12-31T12:00:00.000Z";
    expect(formatDateSpan(start, end, "UTC")).toBe(
      `${formatDay(start, "UTC")} – ${formatDay(end, "UTC")}`,
    );
  });
});

describe("formatUnixDate", () => {
  it("returns Never for missing or invalid values", () => {
    expect(formatUnixDate(null)).toBe("Never");
    expect(formatUnixDate("")).toBe("Never");
    expect(formatUnixDate("0")).toBe("Never");
    expect(formatUnixDate("abc")).toBe("Never");
  });

  it("formats positive unix seconds", () => {
    expect(formatUnixDate("1700000000")).toBe(new Date(1700000000 * 1000).toLocaleDateString());
  });
});

describe("formatIsoDateOnly", () => {
  it("returns null for empty input", () => {
    expect(formatIsoDateOnly(null, "UTC")).toBeNull();
    expect(formatIsoDateOnly(undefined, "UTC")).toBeNull();
  });

  it("formats parseable ISO as YYYY-MM-DD in the zone", () => {
    expect(formatIsoDateOnly("2024-09-09T15:30:00.000Z", "UTC")).toBe("2024-09-09");
  });

  it("extracts leading date from unparseable strings", () => {
    expect(formatIsoDateOnly("2024-09-09", "UTC")).toBe("2024-09-09");
  });
});
