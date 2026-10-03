import { describe, expect, it } from "vitest";
import { lightboxImages } from "./lightboxImages";
import type { Message, MessageAttachment } from "./types";

function photo(name: string): MessageAttachment {
  return { original_name: name, mime_type: "image/jpeg", sha256: "aaa", path: name };
}

function message(id: number, attachments: MessageAttachment[]): Message {
  return {
    id,
    source: "imessage",
    service: "iMessage",
    guid: null,
    timestamp: "2026-08-11T15:04:00Z",
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: id,
    sender: "+1555",
    subject: null,
    text: null,
    attachments,
    tapbacks: [],
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      group_title: null,
      participants: [{ identity: "+1555", name: "Ada", contact_id: null }],
    },
  };
}

describe("lightboxImages", () => {
  it("opens at the image that was clicked when two messages carry the same photo", () => {
    const first = photo("first.jpg");
    const second = photo("second.jpg");
    const messages = [message(1, [first]), message(2, [second])];

    const { items, index } = lightboxImages(messages, second);

    expect(items).toEqual([first, second]);
    expect(index).toBe(1);
  });

  it("shows the clicked attachment alone when no loaded image matches", () => {
    const clicked = photo("only.jpg");

    expect(lightboxImages([], clicked)).toEqual({ items: [clicked], index: 0 });
  });
});
