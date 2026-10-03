/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Message } from "../../lib/types";
import MessageThread from "./MessageThread";

afterEach(() => {
  cleanup();
});

function message(partial: Partial<Message> = {}): Message {
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
    sender: "+1555",
    subject: null,
    text: "hi",
    attachments: [],
    tapbacks: [],
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      group_title: null,
      participants: [{ identity: "+1555", name: "Ada", contact_id: null }],
    },
    ...partial,
  };
}

const baseProps = {
  loading: false,
  error: null as unknown,
  findTerm: "",
  highlightId: null as number | null,
  isGroup: false,
  hasOlder: false,
  hasNewer: false,
  loadingOlder: false,
  loadingNewer: false,
  onLoadOlder: vi.fn(),
  onLoadNewer: vi.fn(),
  landing: { seq: 0, to: "bottom" as const },
  onAttachmentClick: vi.fn(),
};

describe("MessageThread", () => {
  it("shows a loading state", () => {
    render(<MessageThread {...baseProps} messages={[]} loading={true} />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("shows the server's error sentence when the query failed", () => {
    render(<MessageThread {...baseProps} messages={[]} error={new Error("server is down")} />);
    expect(screen.getByText("server is down")).toBeInTheDocument();
  });

  it("falls back to a generic message when the error carries no message", () => {
    render(<MessageThread {...baseProps} messages={[]} error={"not an Error"} />);
    expect(screen.getByText("Could not load messages.")).toBeInTheDocument();
  });

  it("shows an empty state when the conversation has no messages", () => {
    render(<MessageThread {...baseProps} messages={[]} />);
    expect(screen.getByText("No messages in this conversation")).toBeInTheDocument();
  });

  it("renders messages when there are some", () => {
    render(<MessageThread {...baseProps} messages={[message()]} />);
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.queryByText("No messages in this conversation")).not.toBeInTheDocument();
  });

  it("names a group's sender at the start of a run only", () => {
    const group = {
      id: 1,
      chat_identifier: "x",
      conversation_type: "group",
      group_title: null,
      participants: [
        { identity: "+15555550101", name: "Ada", contact_id: null },
        { identity: "+15555550102", name: "Bo", contact_id: null },
      ],
    };
    const from = (id: number, sender: string, timestamp: string) =>
      message({ id, sender, timestamp, text: `m${id}`, conversation: group });
    render(
      <MessageThread
        {...baseProps}
        isGroup={true}
        messages={[
          from(1, "+15555550101", "2026-08-11T15:00:00Z"),
          from(2, "+15555550101", "2026-08-11T15:05:00Z"),
          from(3, "+15555550102", "2026-08-11T15:06:00Z"),
        ]}
      />,
    );
    expect(screen.getAllByText("Ada")).toHaveLength(1);
    expect(screen.getAllByText("Bo")).toHaveLength(1);
  });

  it("says older messages load by scrolling, with no Previous or Next", () => {
    render(<MessageThread {...baseProps} messages={[message()]} hasOlder={true} />);
    expect(screen.getByText("Scroll up for older messages")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Previous" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Next" })).toBeNull();
  });
});
