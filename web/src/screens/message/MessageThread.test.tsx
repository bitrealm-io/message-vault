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
      participants: [{ handle: "+1555", name: "Ada", contact_id: null }],
    },
    ...partial,
  };
}

const baseProps = {
  loading: false,
  error: null as unknown,
  findTerm: "",
  matchIds: [] as string[],
  activeMatch: 0,
  activeYear: null as number | null,
  footerLabel: "Messages 0 of 0",
  offset: 0,
  total: 0,
  onPrevPage: vi.fn(),
  onNextPage: vi.fn(),
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

  it("names the year in the empty state while a year filter is on", () => {
    render(<MessageThread {...baseProps} messages={[]} activeYear={2021} />);
    expect(screen.getByText("No messages in 2021")).toBeInTheDocument();
  });

  it("renders messages when there are some", () => {
    render(<MessageThread {...baseProps} messages={[message()]} />);
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.queryByText("No messages in this conversation")).not.toBeInTheDocument();
  });
});
