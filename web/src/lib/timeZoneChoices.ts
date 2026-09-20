import { getTimeZones } from "@vvo/tzdb";

/**
 * The rows of the time zone picker.
 *
 * An IANA name ("America/Indiana/Knox") is what the vault stores and not what
 * a person looks for. A row is read as "(UTC−05:00) Central Time — Chicago,
 * Houston" and found by typing any of it, a city, a country, an abbreviation
 * or the IANA name itself. The zones, their cities and their countries are
 * `@vvo/tzdb`'s; the offset is the one in force now, so it follows daylight
 * saving.
 */
export interface TimeZoneChoice {
  /** The IANA name sent to the vault. */
  id: string;
  label: string;
  /** Minutes east of UTC now; the rows are ordered by it. */
  offsetMinutes: number;
  /** Everything the row is found by, folded for matching. */
  haystack: string;
  /** Every IANA name that means this zone, `id` among them. */
  names: string[];
}

/** Lower case, without accents, with a plain hyphen for the minus sign. */
function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/\u2212/g, "-")
    .toLowerCase();
}

/** "UTC−05:00", "UTC+05:30", "UTC+00:00". */
export function formatUtcOffset(minutes: number): string {
  const sign = minutes < 0 ? "\u2212" : "+";
  const abs = Math.abs(minutes);
  const hh = String(Math.floor(abs / 60)).padStart(2, "0");
  const mm = String(abs % 60).padStart(2, "0");
  return `UTC${sign}${hh}:${mm}`;
}

/** "utc-5 gmt-5", or "utc+5:30 gmt+5:30": the short forms a person types. */
function shortOffsets(minutes: number): string {
  const sign = minutes < 0 ? "-" : "+";
  const abs = Math.abs(minutes);
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  const short = m === 0 ? `${sign}${h}` : `${sign}${h}:${String(m).padStart(2, "0")}`;
  return `utc${short} gmt${short}`;
}

let cached: TimeZoneChoice[] | null = null;

/** Every zone, west to east and then by name. Built once a session. */
export function timeZoneChoices(): TimeZoneChoice[] {
  if (cached) return cached;
  cached = getTimeZones({ includeUtc: true })
    .map((z): TimeZoneChoice => {
      const cities = z.mainCities.filter(Boolean);
      const place = cities.slice(0, 2).join(", ");
      const offset = formatUtcOffset(z.currentTimeOffsetInMinutes);
      const label = `(${offset}) ${z.alternativeName}${place ? ` \u2014 ${place}` : ""}`;
      const names = [z.name, ...z.group.filter((n) => n !== z.name)];
      const haystack = fold(
        [
          label,
          shortOffsets(z.currentTimeOffsetInMinutes),
          z.abbreviation,
          z.countryName,
          z.continentName,
          ...cities,
          ...names,
          ...names.map((n) => n.replaceAll("_", " ")),
        ].join(" "),
      );
      return { id: z.name, label, offsetMinutes: z.currentTimeOffsetInMinutes, haystack, names };
    })
    .sort((a, b) => a.offsetMinutes - b.offsetMinutes || a.label.localeCompare(b.label));
  return cached;
}

/**
 * The row a stored zone belongs to. A zone saved under another of the row's
 * names ("UTC", "US/Central") is that row. A name no row knows gets a row of
 * its own, so a saved zone is never shown as blank.
 */
export function choiceForZone(zone: string): TimeZoneChoice {
  const found = timeZoneChoices().find((c) => c.names.includes(zone));
  if (found) return found;
  return { id: zone, label: zone, offsetMinutes: 0, haystack: fold(zone), names: [zone] };
}

/** The rows that hold every word of `query`; all of them for an empty one. */
export function searchTimeZones(query: string): TimeZoneChoice[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  const all = timeZoneChoices();
  if (words.length === 0) return all;
  return all.filter((c) => words.every((w) => c.haystack.includes(w)));
}
