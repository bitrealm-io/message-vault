/**
 * Calendar date of a message instant, read in `zone` (e.g. "Sep 9, 2024").
 *
 * The zone is the account's (`useTimeZone`), never the browser's: the same
 * instant is a different day in Sydney and in New York, and the day the
 * person expects is the one their account is set to.
 */
export function formatDay(iso: string, zone: string): string {
  return new Date(iso).toLocaleDateString([], {
    timeZone: zone,
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * The clock time under a bubble, read in `zone` (e.g. "8:36 AM"). The day is
 * the separator's above it, so the bubble carries only the time.
 */
export function formatClockTime(timestamp: string, zone: string): string {
  return new Date(timestamp).toLocaleTimeString([], {
    timeZone: zone,
    hour: "numeric",
    minute: "2-digit",
  });
}

/** Month and year of a message instant in `zone` (e.g. "Sep 2024"). */
export function formatMonthYear(iso: string, zone: string): string {
  return new Date(iso).toLocaleDateString([], {
    timeZone: zone,
    month: "short",
    year: "numeric",
  });
}

/** Locale date + time for import history rows, in the browser's zone. */
export function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * First and last message dates as a span, in `zone`.
 * Returns a single day when start and end format identically.
 */
export function formatDateSpan(
  start: string | null | undefined,
  end: string | null | undefined,
  zone: string,
): string | null {
  if (start && end) {
    const a = formatDay(start, zone);
    const b = formatDay(end, zone);
    return a === b ? a : `${a} – ${b}`;
  }
  if (end) return formatDay(end, zone);
  if (start) return formatDay(start, zone);
  return null;
}

/** Unix seconds string → locale date, or "Never" when missing/invalid. */
export function formatUnixDate(secs: string | null | undefined): string {
  if (secs == null || secs === "") return "Never";
  const n = Number(secs);
  if (!Number.isFinite(n) || n <= 0) return "Never";
  try {
    return new Date(n * 1000).toLocaleDateString();
  } catch {
    return "Never";
  }
}

/**
 * Calendar date of a message instant as YYYY-MM-DD, read in `zone`, or null
 * when unparseable.
 *
 * The zone is the account's (`useTimeZone`), as for `formatDay`, so the day
 * is the one the contact list and the conversation show.
 */
export function formatIsoDateOnly(iso: string | null | undefined, zone: string): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    const m = iso.match(/^(\d{4}-\d{2}-\d{2})/);
    return m ? m[1] : null;
  }
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: zone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(d);
  const part = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((p) => p.type === type)?.value ?? "";
  return `${part("year")}-${part("month")}-${part("day")}`;
}
