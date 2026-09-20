import { createContext, useContext } from "react";

/**
 * The account's time zone, and the one place the web app reads it.
 *
 * A message is stored as an instant, in UTC. It says nothing about where the
 * phone was, so turning it into a clock reading, a day, or a year needs a zone,
 * and that zone is the account's: chosen at profile setup, changed under
 * Settings → Profile, and the same one the vault uses for `date:` boundaries
 * and the year filter. Every label in the app reads it through `useTimeZone`,
 * so a message at 11:59 pm on New Year's Eve sits in the old year in the
 * thread, in the year chips, and in search alike.
 */

/** The zone this browser is in, or UTC when the runtime will not say. */
export function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

/** True when this runtime can format dates in `zone`. */
export function knowsTimeZone(zone: string): boolean {
  try {
    Intl.DateTimeFormat(undefined, { timeZone: zone });
    return true;
  } catch {
    return false;
  }
}

/** The calendar year `iso` falls in, read in `zone`; `NaN` when unparseable. */
export function yearIn(iso: string, zone: string): number {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return Number.NaN;
  return Number(new Intl.DateTimeFormat("en-US", { timeZone: zone, year: "numeric" }).format(d));
}

export const TimeZoneContext = createContext<string | null>(null);

/** The zone to show message times in. Outside a provider: the browser's. */
export function useTimeZone(): string {
  return useContext(TimeZoneContext) ?? browserTimeZone();
}
