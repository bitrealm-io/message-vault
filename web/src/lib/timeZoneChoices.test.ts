import { describe, expect, it } from "vitest";
import {
  choiceForZone,
  formatUtcOffset,
  searchTimeZones,
  timeZoneChoices,
} from "./timeZoneChoices";

const ids = (query: string) => searchTimeZones(query).map((c) => c.id);

describe("timeZoneChoices", () => {
  it("labels a row with its offset, its name and its main cities", () => {
    const chicago = timeZoneChoices().find((c) => c.id === "America/Chicago");
    expect(chicago?.label).toMatch(/^\(UTC\u22120[56]:00\) Central Time \u2014 Chicago, Houston$/);
  });

  it("orders the rows west to east", () => {
    const offsets = timeZoneChoices().map((c) => c.offsetMinutes);
    expect(offsets).toEqual([...offsets].sort((a, b) => a - b));
  });

  it("covers every zone this runtime knows", () => {
    const known = new Set(timeZoneChoices().flatMap((c) => c.names));
    const missing = Intl.supportedValuesOf("timeZone").filter((z) => !known.has(z));
    expect(missing).toEqual([]);
  });
});

describe("searchTimeZones", () => {
  it("finds a zone by a city that is not in its IANA name", () => {
    expect(ids("dallas")).toContain("America/Chicago");
  });

  it("finds zones by country, by abbreviation and by IANA name", () => {
    expect(ids("germany")).toContain("Europe/Berlin");
    expect(ids("cst")).toContain("America/Chicago");
    expect(ids("america/new_york")).toContain("America/New_York");
    expect(ids("new york")).toContain("America/New_York");
  });

  it("matches every word, in any order, without accents", () => {
    expect(ids("time central chicago")).toEqual(["America/Chicago"]);
    expect(ids("sao paulo")).toContain("America/Sao_Paulo");
  });

  it("finds an offset typed with a plain hyphen", () => {
    const kolkata = timeZoneChoices().find((c) => c.id === "Asia/Kolkata");
    expect(kolkata).toBeDefined();
    expect(ids("utc+5:30")).toContain("Asia/Kolkata");
    expect(ids("utc-10")).toContain("Pacific/Honolulu");
  });

  it("returns every row for an empty query and none for nonsense", () => {
    expect(searchTimeZones("  ")).toHaveLength(timeZoneChoices().length);
    expect(searchTimeZones("qqqzzz")).toEqual([]);
  });
});

describe("choiceForZone", () => {
  it("maps another name for a zone to that zone's row", () => {
    expect(choiceForZone("UTC").id).toBe("Etc/UTC");
    expect(choiceForZone("US/Central").id).toBe("America/Chicago");
    expect(choiceForZone("America/Indiana/Knox").id).toBe("America/Chicago");
  });

  it("gives a zone no row knows a row of its own", () => {
    expect(choiceForZone("Mars/Olympus_Mons")).toMatchObject({
      id: "Mars/Olympus_Mons",
      label: "Mars/Olympus_Mons",
    });
  });
});

describe("formatUtcOffset", () => {
  it("writes hours and minutes with a sign", () => {
    expect(formatUtcOffset(-300)).toBe("UTC\u221205:00");
    expect(formatUtcOffset(330)).toBe("UTC+05:30");
    expect(formatUtcOffset(0)).toBe("UTC+00:00");
  });
});
