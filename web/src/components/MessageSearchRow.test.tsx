/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TimeZoneContext } from "../lib/timeZone";
import type { Message } from "../lib/types";
import MessageSearchRow from "./MessageSearchRow";

afterEach(cleanup);

function message(over: Partial<Message> = {}): Message {
  return {
    id: 10,
    source: "imessage",
    // 03:30 UTC on 2 January is still 1 January in New York.
    timestamp: "2024-01-02T03:30:00Z",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sender: "+15555550100",
    text: "Here is the photo from the dentist",
    conversation: {
      id: 1,
      chat_identifier: "chat1",
      conversation_type: "group",
      group_title: "Family",
      participants: [
        { name: "Alice", handle: "+15555550100" },
        { name: "Bob", handle: "+15555550200" },
      ],
    },
    attachments: [],
    tapbacks: [],
    ...over,
  };
}

function renderRow(m: Message, terms = [{ text: "photo", prefix: false }]) {
  return render(
    <TimeZoneContext.Provider value="America/New_York">
      <MessageSearchRow message={m} terms={terms} isSelected={false} onClick={() => {}} />
    </TimeZoneContext.Provider>,
  );
}

describe("MessageSearchRow", () => {
  it("shows the conversation, the day in the account's zone, the sender, and the matching word in bold", () => {
    renderRow(message());
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Family");
    expect(row).toHaveTextContent(
      new Date("2024-01-01T12:00:00Z").toLocaleDateString([], {
        year: "numeric",
        month: "short",
        day: "numeric",
      }),
    );
    expect(row).toHaveTextContent("Alice: Here is the photo from the dentist");
    const bold = row.querySelectorAll("strong");
    expect([...bold].map((b) => b.textContent)).toEqual(["photo"]);
  });

  it('says "You" for a sent message, and counts its attachments', () => {
    renderRow(
      message({
        is_from_me: true,
        sender: null,
        attachments: [{ original_name: "a.jpg" }, { original_name: "b.jpg" }],
      }),
    );
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("You:");
    expect(screen.getByTitle("2 attachments")).toHaveTextContent("📎 2");
  });

  it("shows no sender for a received message that names none", () => {
    renderRow(message({ sender: null }));
    const row = screen.getByRole("button");
    expect(row).not.toHaveTextContent("Unknown");
    expect(row).not.toHaveTextContent(":");
  });

  it("shows the attachments' names for a message with no text, with a matching name in bold", () => {
    renderRow(message({ text: null, attachments: [{ original_name: "photo 1.jpg" }] }));
    const row = screen.getByRole("button");
    expect(row).toHaveTextContent("Alice: photo 1.jpg");
    expect(row.querySelector("strong")?.textContent).toBe("photo");
  });
});
