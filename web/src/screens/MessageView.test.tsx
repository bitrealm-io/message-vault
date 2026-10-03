/** @vitest-environment jsdom */

/**
 * Find in conversation steps through every match the server counted, not only
 * the page of them on screen (issue #1145).
 */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  getAccountProfile,
  listContactGroups,
  listConversationMessages,
  listMessages,
} from "../lib/serverApi";
import type { Conversation, Message } from "../lib/types";
import { mockedAuth, Providers } from "../test/providers";
import MessageView from "./MessageView";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  getAccountProfile: vi.fn(),
  listContactGroups: vi.fn(),
  listConversationMessages: vi.fn(),
  listMessages: vi.fn(),
}));

/** Matches in the conversation, more than two pages of 50. */
const MATCHES = 120;

function message(id: number): Message {
  return {
    id,
    source: "test",
    service: "sms",
    guid: null,
    timestamp: "2024-01-01T00:00:00Z",
    is_from_me: false,
    sender: "someone",
    subject: null,
    text: `hello ${id}`,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: id,
    conversation: {
      id: 42,
      chat_identifier: "c",
      conversation_type: "direct",
      group_title: null,
      participants: [],
    },
    attachments: [],
    tapbacks: [],
  };
}

const conversation: Conversation = {
  id: 42,
  participants: [],
  message_count: MATCHES,
  last_message_at: "",
  date_range_start: null,
  date_range_end: null,
  service: "sms",
  is_group: false,
  label: "Chat 42",
  tags: [],
};

beforeAll(() => {
  // jsdom has no layout, so it has no scrolling either.
  Element.prototype.scrollIntoView = () => {};
});

beforeEach(() => {
  vi.mocked(getAccountProfile).mockReturnValue(new Promise(() => {}));
  vi.mocked(listContactGroups).mockResolvedValue([]);
  vi.mocked(listConversationMessages).mockResolvedValue({
    items: [],
    total: 0,
    limit: 50,
    offset: 0,
  });
  // The server pages the matches 50 at a time and counts all of them.
  vi.mocked(listMessages).mockImplementation(async ({ offset = 0, limit = 50 }) => {
    const ids = Array.from({ length: Math.max(0, Math.min(limit, MATCHES - offset)) }, (_, i) =>
      message(offset + i + 1),
    );
    return { items: ids, total: MATCHES, limit, offset };
  });
});

afterEach(cleanup);

/** The thread has drawn message `id`: the highlight splits its text, so look for its row. */
async function shown(id: number) {
  await waitFor(() => expect(document.getElementById(`msg-${id}`)).not.toBeNull());
}

async function findHello() {
  const user = userEvent.setup({ delay: null });
  render(
    <Providers>
      <MemoryRouter>
        <MessageView conversation={conversation} />
      </MemoryRouter>
    </Providers>,
  );
  await user.type(screen.getByPlaceholderText("Find in conversation…"), "hello");
  await screen.findByText("1 of 120 in this conversation");
  return user;
}

describe("MessageView find", () => {
  it("moves to the next page from the last match of a page", async () => {
    const user = await findHello();
    for (let i = 0; i < 49; i++) await user.click(screen.getByRole("button", { name: "↓" }));
    await screen.findByText("50 of 120 in this conversation");

    await user.click(screen.getByRole("button", { name: "↓" }));

    await screen.findByText("51 of 120 in this conversation");
    await waitFor(() =>
      expect(vi.mocked(listMessages)).toHaveBeenCalledWith(
        expect.objectContaining({ offset: 50 }),
        expect.anything(),
      ),
    );
    await shown(51);
  });

  it("goes back from the first match to the last of all of them, on the last page", async () => {
    const user = await findHello();

    await user.click(screen.getByRole("button", { name: "↑" }));

    await screen.findByText("120 of 120 in this conversation");
    await shown(120);
  });

  it("goes back to the previous page from the first match of a page", async () => {
    const user = await findHello();
    await user.click(screen.getByRole("button", { name: "↑" }));
    await shown(120);

    await user.click(screen.getByRole("button", { name: "↓" }));
    await screen.findByText("1 of 120 in this conversation");
    await shown(1);
  });
});
