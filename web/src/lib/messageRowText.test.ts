import { describe, expect, it } from "vitest";
import { messageConversationName, messageRowText, messageSenderName } from "./messageRowText";
import type { Message, MessageConversation } from "./types";

function conversation(over: Partial<MessageConversation> = {}): MessageConversation {
  return {
    id: 1,
    chat_identifier: "+15555550100",
    conversation_type: "individual",
    group_title: null,
    participants: [{ name: "Alice", handle: "+15555550100", contact_id: 4, service: "imessage" }],
    ...over,
  };
}

function message(over: Partial<Message> = {}): Message {
  return {
    id: 10,
    source: "imessage",
    timestamp: "2024-01-01T10:00:00Z",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sender: "+15555550100",
    text: "hello",
    conversation: conversation(),
    attachments: [],
    tapbacks: [],
    ...over,
  };
}

describe("messageConversationName", () => {
  it("names a one-to-one conversation after the other person", () => {
    expect(messageConversationName(conversation())).toBe("Alice");
  });

  it("names a group conversation by its title, else by everyone in it", () => {
    const people = [
      { name: "Alice", handle: "+1" },
      { name: "Bob", handle: "+2" },
    ];
    expect(
      messageConversationName(
        conversation({ conversation_type: "group", group_title: " Family ", participants: people }),
      ),
    ).toBe("Family");
    expect(
      messageConversationName(conversation({ conversation_type: "group", participants: people })),
    ).toBe("Alice, Bob");
    // The server reads the type ignoring case, and so does the row.
    expect(
      messageConversationName(conversation({ conversation_type: "Group", participants: people })),
    ).toBe("Alice, Bob");
  });
});

describe("messageSenderName", () => {
  it('is "You" for a message the account sent', () => {
    expect(messageSenderName(message({ is_from_me: true, sender: null }))).toBe("You");
  });

  it("is the participant's name for their identity, else the identity", () => {
    expect(messageSenderName(message())).toBe("Alice");
    expect(messageSenderName(message({ sender: "+15555550999" }))).toBe("+15555550999");
  });

  it("is null for a received message that names no sender", () => {
    expect(messageSenderName(message({ sender: null }))).toBeNull();
  });
});

describe("messageRowText", () => {
  it("is the message's text, or its attachments' names when it has none", () => {
    expect(messageRowText(message())).toBe("hello");
    expect(
      messageRowText(
        message({
          text: null,
          attachments: [{ original_name: "IMG_0001.jpg" }, { original_name: "notes.pdf" }],
        }),
      ),
    ).toBe("IMG_0001.jpg, notes.pdf");
  });
});
