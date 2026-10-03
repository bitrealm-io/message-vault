import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { annotateMessageRuns, messagesFormRun } from "./messageRun";
import type { MessageRow } from "./types";

function msg(
  partial: Partial<MessageRow> & Pick<MessageRow, "id" | "timestamp">,
): MessageRow {
  return {
    source: "imessage",
    isFromMe: false,
    sender: "+15555550119",
    senderName: "Alex",
    body: "hi",
    isAnnouncement: false,
    attachments: [],
    ...partial,
  };
}

describe("messagesFormRun", () => {
  it("groups same sender within five minutes on the same day", () => {
    const a = msg({ id: 1, timestamp: "2024-06-01T12:00:00Z" });
    const b = msg({ id: 2, timestamp: "2024-06-01T12:04:00Z" });
    assert.equal(messagesFormRun(a, b), true);
  });

  it("breaks on sender change", () => {
    const a = msg({ id: 1, timestamp: "2024-06-01T12:00:00Z", sender: "a" });
    const b = msg({
      id: 2,
      timestamp: "2024-06-01T12:01:00Z",
      sender: "b",
      senderName: "Blake",
    });
    assert.equal(messagesFormRun(a, b), false);
  });

  it("breaks on direction change", () => {
    const a = msg({ id: 1, timestamp: "2024-06-01T12:00:00Z", isFromMe: false });
    const b = msg({ id: 2, timestamp: "2024-06-01T12:01:00Z", isFromMe: true });
    assert.equal(messagesFormRun(a, b), false);
  });

  it("breaks after a five-minute gap", () => {
    const a = msg({ id: 1, timestamp: "2024-06-01T12:00:00Z" });
    const b = msg({ id: 2, timestamp: "2024-06-01T12:06:00Z" });
    assert.equal(messagesFormRun(a, b), false);
  });

  it("breaks across calendar days", () => {
    const a = msg({ id: 1, timestamp: "2024-06-01T23:59:00Z" });
    const b = msg({ id: 2, timestamp: "2024-06-02T00:01:00Z" });
    assert.equal(messagesFormRun(a, b), false);
  });

  it("never groups announcements", () => {
    const a = msg({
      id: 1,
      timestamp: "2024-06-01T12:00:00Z",
      isAnnouncement: true,
    });
    const b = msg({ id: 2, timestamp: "2024-06-01T12:01:00Z" });
    assert.equal(messagesFormRun(a, b), false);
  });
});

describe("annotateMessageRuns", () => {
  it("marks first/middle/last and controls sender/timestamp visibility", () => {
    const items = annotateMessageRuns([
      msg({ id: 1, timestamp: "2024-06-01T12:00:00Z" }),
      msg({ id: 2, timestamp: "2024-06-01T12:01:00Z" }),
      msg({ id: 3, timestamp: "2024-06-01T12:02:00Z" }),
    ]);
    assert.deepEqual(
      items.map((i) => ({
        id: i.message.id,
        run: i.run,
        showSender: i.showSender,
        showTimestamp: i.showTimestamp,
      })),
      [
        { id: 1, run: "first", showSender: true, showTimestamp: false },
        { id: 2, run: "middle", showSender: false, showTimestamp: false },
        { id: 3, run: "last", showSender: false, showTimestamp: true },
      ],
    );
  });

  it("marks isolated messages as single", () => {
    const items = annotateMessageRuns([
      msg({ id: 1, timestamp: "2024-06-01T12:00:00Z" }),
      msg({
        id: 2,
        timestamp: "2024-06-01T12:10:00Z",
        isFromMe: true,
        sender: null,
        senderName: "Me",
      }),
    ]);
    assert.equal(items[0]!.run, "single");
    assert.equal(items[0]!.showSender, true);
    assert.equal(items[0]!.showTimestamp, true);
    assert.equal(items[1]!.run, "single");
    assert.equal(items[1]!.showSender, false);
    assert.equal(items[1]!.showTimestamp, true);
  });
});
