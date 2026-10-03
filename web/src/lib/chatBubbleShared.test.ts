/** @vitest-environment jsdom */
import { beforeEach, describe, expect, it } from "vitest";
import {
  bubbleBody,
  formatMessageTime,
  isGroupConversation,
  senderName,
  tapbackGroups,
} from "../components/messages/chatBubbleShared";
import type { Message, MessageTapback } from "./types";

function message(partial: Partial<Message> & Pick<Message, "conversation">): Message {
  return {
    id: 1,
    source: "imessage",
    service: "iMessage",
    guid: null,
    timestamp: "2026-08-11T15:04:00Z",
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: 0,
    sender: null,
    subject: null,
    text: "hi",
    attachments: [],
    tapbacks: [],
    ...partial,
  };
}

describe("formatMessageTime", () => {
  it("returns a non-empty locale string", () => {
    const s = formatMessageTime("2026-08-11T15:04:00Z", "UTC");
    expect(s.length).toBeGreaterThan(0);
    expect(s).not.toBe("Invalid Date");
  });

  it("reads the clock in the account's zone", () => {
    // The server stores the instant; the zone decides what the clock said.
    const instant = "2026-08-11T15:04:00Z";
    expect(formatMessageTime(instant, "America/New_York")).toMatch(/11:04/);
    expect(formatMessageTime(instant, "Europe/Paris")).toMatch(/17:04|5:04/);
  });
});

describe("bubbleBody", () => {
  it("returns undefined for empty body", () => {
    expect(bubbleBody("", undefined)).toBeUndefined();
  });

  it("returns plain text without highlight", () => {
    expect(bubbleBody("hello", undefined)).toBe("hello");
  });

  it("returns highlighted nodes when a term is set", () => {
    const nodes = bubbleBody("hello world", "hello");
    expect(Array.isArray(nodes)).toBe(true);
    expect(nodes?.length).toBeGreaterThan(0);
  });
});

describe("senderName / isGroupConversation", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("labels outbound messages as Me", () => {
    const m = message({
      is_from_me: true,
      conversation: {
        id: 1,
        chat_identifier: "x",
        conversation_type: "individual",
        group_title: null,
        participants: [{ identity: "+1", name: "Ada", contact_id: null }],
      },
    });
    expect(senderName(m)).toBe("Me");
  });

  it("uses the participant's server-supplied name", () => {
    const conversation = {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      group_title: null,
      participants: [
        {
          identity: "+1555",
          name: "Ada",
          contact_id: null,
        },
      ],
    };
    const m = message({ sender: "+1555", conversation });
    expect(senderName(m)).toBe("Ada");
  });

  it("detects groups from type or participant count", () => {
    const one = message({
      conversation: {
        id: 1,
        chat_identifier: "x",
        conversation_type: "individual",
        group_title: null,
        participants: [{ identity: "a", name: "A", contact_id: null }],
      },
    });
    expect(isGroupConversation(one)).toBe(false);

    const typed = message({
      conversation: {
        ...one.conversation,
        conversation_type: "group",
      },
    });
    expect(isGroupConversation(typed)).toBe(true);

    const many = message({
      conversation: {
        id: 1,
        chat_identifier: "x",
        conversation_type: "individual",
        group_title: null,
        participants: [
          { identity: "a", name: "A", contact_id: null },
          { identity: "b", name: "B", contact_id: null },
        ],
      },
    });
    expect(isGroupConversation(many)).toBe(true);
  });
});

function tapback(partial: Partial<MessageTapback>): MessageTapback {
  return {
    emoji: null,
    is_from_me: false,
    kind: "loved",
    part_index: 0,
    sender: null,
    ...partial,
  };
}

const conversation = {
  id: 1,
  chat_identifier: "x",
  conversation_type: "group",
  group_title: null,
  participants: [
    { identity: "+1555", name: "Ada", contact_id: null },
    { identity: "+1556", name: "Bob", contact_id: null },
  ],
};

describe("tapbackGroups", () => {
  it("maps an emoji-less iMessage kind to its fixed emoji", () => {
    const m = message({ conversation, tapbacks: [tapback({ kind: "loved", sender: "+1555" })] });
    expect(tapbackGroups(m)).toEqual([{ emoji: "❤️", count: 1, senderNames: ["Ada"] }]);
  });

  it("prefers the tapback's own emoji over the fixed kind mapping", () => {
    const m = message({
      conversation,
      tapbacks: [tapback({ kind: "loved", emoji: "🔥", sender: "+1555" })],
    });
    expect(tapbackGroups(m)[0]?.emoji).toBe("🔥");
  });

  it("groups by emoji and counts one entry per reactor", () => {
    const m = message({
      conversation,
      tapbacks: [
        tapback({ kind: "loved", sender: "+1555" }),
        tapback({ kind: "loved", sender: "+1556" }),
      ],
    });
    expect(tapbackGroups(m)).toEqual([{ emoji: "❤️", count: 2, senderNames: ["Ada", "Bob"] }]);
  });

  // The exporter's kind vocabulary ends `emoji|sticker`. A sticker tapback
  // carries no emoji of its own, so without a glyph the badge showed the
  // literal word "sticker".
  it("gives a sticker tapback a glyph rather than the word", () => {
    const m = message({ conversation, tapbacks: [tapback({ kind: "sticker", sender: "+1555" })] });
    expect(tapbackGroups(m)[0]?.emoji).toBe("🖼️");
  });

  it("names the account owner Me", () => {
    const m = message({ conversation, tapbacks: [tapback({ kind: "liked", is_from_me: true })] });
    expect(tapbackGroups(m)[0]?.senderNames).toEqual(["Me"]);
  });

  it("is empty when the message has no tapbacks", () => {
    const m = message({ conversation, tapbacks: [] });
    expect(tapbackGroups(m)).toEqual([]);
  });
});
