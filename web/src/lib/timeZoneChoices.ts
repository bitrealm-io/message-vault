import { getTimeZones } from "@vvo/tzdb";

/**
 * The rows of the time zone picker.
 *
 * An IANA name ("America/Chicago") is what the server stores and not what a
 * person looks for. A group's row is read as "(UTC−05:00) Central Time —
 * Chicago, Houston" and found by typing any of it, a city, a country, an
 * abbreviation or the IANA name itself. The zones, their groups, cities and
 * countries are `@vvo/tzdb`'s; the offset is the one in force now, so it
 * follows daylight saving.
 */
export interface TimeZoneChoice {
  /** The IANA name sent to the server. */
  id: string;
  label: string;
  /** Minutes east of UTC now; the rows are ordered by it. */
  offsetMinutes: number;
  /** Everything the row is found by, folded for matching. */
  haystack: string;
  /** For a group's row, every IANA name of the group, `id` first; else `[id]`. */
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

/** West to east, then by label. */
function westToEast(a: TimeZoneChoice, b: TimeZoneChoice): number {
  return a.offsetMinutes - b.offsetMinutes || a.label.localeCompare(b.label);
}

interface Choices {
  /** One row for each `@vvo/tzdb` group, under its representative's name. */
  groups: TimeZoneChoice[];
  /** One row for every other name of a group, found only by that name. */
  members: TimeZoneChoice[];
  /** Every row of both lists, by its `id`. */
  byId: Map<string, TimeZoneChoice>;
}

let cached: Choices | null = null;

/**
 * The rows, built once a session.
 *
 * The zones of a `@vvo/tzdb` group share today's rules, not their history:
 * America/Indiana/Knox kept Eastern time from 1991 to 2006 and America/Chicago
 * did not. A group's row therefore stands for its representative alone, and
 * each other zone of the group has a row of its own, labelled with its name,
 * so the zone a person finds is the zone that is stored.
 */
function choices(): Choices {
  if (cached) return cached;
  const zones = getTimeZones({ includeUtc: true });
  const groups: TimeZoneChoice[] = [];
  const members: TimeZoneChoice[] = [];
  // A group can list another group's representative ("America/Cayman" under
  // "America/Panama"), and two groups can list the same zone
  // ("Europe/Busingen"). Each name gets one row, its own group's if it has one.
  const named = new Set(zones.map((z) => z.name));
  for (const z of zones) {
    const offsetMinutes = z.currentTimeOffsetInMinutes;
    const offset = formatUtcOffset(offsetMinutes);
    const cities = z.mainCities.filter(Boolean);
    const place = cities.slice(0, 2).join(", ");
    const label = `(${offset}) ${z.alternativeName}${place ? ` \u2014 ${place}` : ""}`;
    const rest = z.group.filter((n) => n !== z.name);
    const haystack = fold(
      [
        label,
        shortOffsets(offsetMinutes),
        z.abbreviation,
        z.countryName,
        z.continentName,
        ...cities,
        z.name,
        z.name.replaceAll("_", " "),
      ].join(" "),
    );
    groups.push({ id: z.name, label, offsetMinutes, haystack, names: [z.name, ...rest] });
    for (const name of rest) {
      if (named.has(name)) continue;
      named.add(name);
      members.push({
        id: name,
        label: `(${offset}) ${name}`,
        offsetMinutes,
        haystack: fold(`${name} ${name.replaceAll("_", " ")}`),
        names: [name],
      });
    }
  }
  groups.sort(westToEast);
  members.sort(westToEast);
  const byId = new Map<string, TimeZoneChoice>();
  for (const c of [...members, ...groups]) byId.set(c.id, c);
  cached = { groups, members, byId };
  return cached;
}

/** One row for each group of zones, west to east and then by name. */
export function timeZoneChoices(): TimeZoneChoice[] {
  return choices().groups;
}

/**
 * The row of a stored zone. A group's representative is its group's row. Any
 * other zone of a group ("America/Indiana/Knox", "UTC") is shown under its own
 * name, so the field shows what is stored. A name no row knows gets a row of
 * its own, so a saved zone is never shown as blank.
 */
export function choiceForZone(zone: string): TimeZoneChoice {
  const found = choices().byId.get(zone);
  if (found) return found;
  return { id: zone, label: zone, offsetMinutes: 0, haystack: fold(zone), names: [zone] };
}

/**
 * The rows that hold every word of `query`, west to east; every group's row
 * for an empty one. A zone that shares its group's row is offered on a row of
 * its own when `query` matches its name.
 */
export function searchTimeZones(query: string): TimeZoneChoice[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  const { groups, members } = choices();
  if (words.length === 0) return groups;
  const matches = (c: TimeZoneChoice) => words.every((w) => c.haystack.includes(w));
  return [...groups.filter(matches), ...members.filter(matches)].sort(westToEast);
}
