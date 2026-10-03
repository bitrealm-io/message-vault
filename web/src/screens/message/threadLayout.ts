import { yearIn } from "../../lib/timeZone";
import type { Message } from "../../lib/types";

/** A gap this long or longer between two messages starts a new run (#1391). */
export const RUN_GAP_MS = 60 * 60 * 1000;

/** The calendar day an instant falls on in `zone`, as `YYYY-MM-DD`, for comparing two days. */
function dayKey(iso: string, zone: string): string {
  return new Intl.DateTimeFormat("en-CA", {
    timeZone: zone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(new Date(iso));
}

/**
 * The day separator's text: "Thu, Jul 2" in the current year, and
 * "Mon, Nov 29, 2021" in any other, read in the account's `zone`.
 */
export function dayLabel(iso: string, zone: string, now: Date = new Date()): string {
  const sameYear = yearIn(iso, zone) === yearIn(now.toISOString(), zone);
  return new Date(iso).toLocaleDateString([], {
    timeZone: zone,
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" as const }),
  });
}

/** One message of the thread, with what is drawn around it. */
export type ThreadRow = {
  message: Message;
  /** The day separator drawn above it, or `null` when it is not the first of its day. */
  day: string | null;
  /**
   * Whether it starts a run: the first message, or one after a new day, a new
   * sender, or a gap of an hour or more. In a group, only a run's first
   * message carries its sender's name.
   */
  startsRun: boolean;
};

/** Whether two messages came from the same person: the account, or one sender. */
function sameSender(a: Message, b: Message): boolean {
  if (a.is_from_me || b.is_from_me) return a.is_from_me === b.is_from_me;
  return a.sender === b.sender;
}

/** The thread's messages, oldest first, with their day separators and runs. */
export function threadRows(messages: Message[], zone: string, now: Date = new Date()): ThreadRow[] {
  let previous: Message | null = null;
  let previousDay = "";
  return messages.map((message) => {
    const day = dayKey(message.timestamp, zone);
    const newDay = previous === null || day !== previousDay;
    const startsRun =
      previous === null ||
      newDay ||
      !sameSender(previous, message) ||
      Date.parse(message.timestamp) - Date.parse(previous.timestamp) >= RUN_GAP_MS;
    previous = message;
    previousDay = day;
    return { message, day: newDay ? dayLabel(message.timestamp, zone, now) : null, startsRun };
  });
}
