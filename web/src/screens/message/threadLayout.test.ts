import { describe, expect, it } from "vitest";
import type { Message } from "../../lib/types";
import { dayLabel, threadRows } from "./threadLayout";

function message(
  id: number,
  timestamp: string,
  from: { me: true } | { sender: string },
  conversationType = "group",
): Message {
  return {
    id,
    source: "imessage",
    service: "iMessage",
    guid: null,
    timestamp,
    is_from_me: "me" in from,
    sender: "sender" in from ? from.sender : null,
    subject: null,
    text: `m${id}`,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: 0,
    conversation: {
      id: 1,
      chat_identifier: "c",
      conversation_type: conversationType,
      group_title: null,
      participants: [],
    },
    attachments: [],
    tapbacks: [],
  };
}

const NOW = new Date("2026-08-01T12:00:00Z");

describe("dayLabel", () => {
  it("leaves the year out in the current year and names it in any other", () => {
    expect(dayLabel("2026-07-02T10:00:00Z", "UTC", NOW)).toBe("Thu, Jul 2");
    expect(dayLabel("2021-11-29T22:34:00Z", "UTC", NOW)).toBe("Mon, Nov 29, 2021");
  });

  it("reads the day in the account's time zone", () => {
    // 02:00 UTC on Jul 3 is still Jul 2 in New York.
    expect(dayLabel("2026-07-03T02:00:00Z", "America/New_York", NOW)).toBe("Thu, Jul 2");
  });
});

describe("threadRows", () => {
  const ada = { sender: "+15555550101" };
  const bo = { sender: "+15555550102" };

  it("puts a day separator before the first message of each day", () => {
    const rows = threadRows(
      [
        message(1, "2026-07-02T08:00:00Z", ada),
        message(2, "2026-07-02T09:00:00Z", ada),
        message(3, "2026-07-03T08:00:00Z", ada),
      ],
      "UTC",
      NOW,
    );
    expect(rows.map((r) => r.day)).toEqual(["Thu, Jul 2", null, "Fri, Jul 3"]);
  });

  it("starts a run at a new day, a new sender, or a gap of an hour or more", () => {
    const rows = threadRows(
      [
        message(1, "2026-07-02T08:00:00Z", ada),
        message(2, "2026-07-02T08:10:00Z", ada),
        message(3, "2026-07-02T08:20:00Z", bo),
        message(4, "2026-07-02T08:30:00Z", { me: true }),
        message(5, "2026-07-02T08:31:00Z", bo),
        message(6, "2026-07-02T09:30:59Z", bo),
        message(7, "2026-07-02T10:31:00Z", bo),
        message(8, "2026-07-03T00:01:00Z", bo),
      ],
      "UTC",
      NOW,
    );
    expect(rows.map((r) => r.startsRun)).toEqual([
      true, // first loaded
      false, // same sender, ten minutes on
      true, // new sender
      true, // the account's own message
      true, // back to bo
      false, // 59 minutes 59 seconds on
      true, // an hour on
      true, // a new day
    ]);
  });
});
