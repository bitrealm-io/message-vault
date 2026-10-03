/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getConversation, listConversationMessages, trashConversation } from "../lib/serverApi";
import type { Conversation, Message } from "../lib/types";
import { mockedAuth, Providers } from "../test/providers";
import MessageRoute from "./MessageRoute";
import { RightToolbarProvider } from "./RightToolbarContext";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  // The sidebar list `MessageRoute` renders alongside the thread.
  listConversations: vi.fn().mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 }),
  listContactGroups: vi.fn().mockResolvedValue({ items: [] }),
  listMessageTags: vi.fn().mockResolvedValue({ items: [] }),
  getConversation: vi.fn(),
  listConversationMessages: vi.fn(),
  trashConversation: vi.fn(),
}));

// The real list virtualizes its rows, and jsdom lays out none. This stand-in
// offers one row to click and shows the query the route hands the list.
vi.mock("../screens/ConversationList", () => ({
  default: ({ onSelect, query }: { onSelect: (c: Conversation) => void; query: string }) => (
    <div>
      <output data-testid="list-query">{query}</output>
      <button type="button" onClick={() => onSelect(conv(6, "Second result"))}>
        Second result
      </button>
    </div>
  ),
}));

// The Messages list, the same way: one result in conversation 6 to click,
// and the query and the selected message the route hands it.
vi.mock("../screens/MessageSearchList", () => ({
  default: ({
    onSelect,
    query,
    selectedId,
  }: {
    onSelect: (m: Message) => void;
    query: string;
    selectedId: number | null;
  }) => (
    <div>
      <output data-testid="message-list-query">{query}</output>
      <output data-testid="message-list-selected">{String(selectedId)}</output>
      <button type="button" onClick={() => onSelect(message(77, 6))}>
        Message result
      </button>
    </div>
  ),
}));

const getConversationMock = vi.mocked(getConversation);
const listConversationMessagesMock = vi.mocked(listConversationMessages);
const trashConversationMock = vi.mocked(trashConversation);

// jsdom has no ResizeObserver; VirtualList observes its scroll container on mount.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  getConversationMock.mockReset();
  listConversationMessagesMock.mockReset();
  trashConversationMock.mockReset();
  listConversationMessagesMock.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
});

afterEach(() => {
  vi.unstubAllGlobals();
  cleanup();
});

function conv(id: number, label: string): Conversation {
  return {
    id,
    participants: [],
    message_count: 1,
    last_message_at: "",
    date_range_start: null,
    date_range_end: null,
    service: "sms",
    is_group: false,
    label,
    tags: [],
  };
}

function message(id: number, conversationId: number): Message {
  return {
    id,
    source: "imessage",
    timestamp: "2024-01-01T10:00:00Z",
    sort_order: 0,
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    text: "photo",
    conversation: {
      id: conversationId,
      chat_identifier: "+1",
      conversation_type: "individual",
      participants: [],
    },
    attachments: [],
    tapbacks: [],
  };
}

function renderAt(path: string, state?: unknown) {
  /** Where the router is now. */
  function LocationProbe() {
    const location = useLocation();
    return <output data-testid="location">{location.pathname + location.search}</output>;
  }

  /**
   * Opens conversation 8 as a click on its row in the list column does: the row
   * rides along as `location.state`, so the thread pane never shows "Loading".
   */
  function OpenEight() {
    const navigate = useNavigate();
    return (
      <button
        type="button"
        onClick={() => navigate("/messages/8", { state: { conversation: conv(8, "Chat 8") } })}
      >
        open 8
      </button>
    );
  }

  const [pathname, search = ""] = path.split(/(?=\?)/);
  return render(
    <Providers>
      <MemoryRouter initialEntries={[{ pathname, search, state }]}>
        <RightToolbarProvider>
          <OpenEight />
          <LocationProbe />
          <Routes>
            <Route path="/" element={<div>Conversations list</div>} />
            <Route path="/messages/:conversationId" element={<MessageRoute />} />
            <Route path="/messages" element={<MessageRoute />} />
          </Routes>
        </RightToolbarProvider>
      </MemoryRouter>
    </Providers>,
  );
}

describe("MessageRoute", () => {
  it("renders the thread for a conversation id in the URL", async () => {
    getConversationMock.mockResolvedValue(conv(5, "Chat 5"));

    renderAt("/messages/5");

    expect(await screen.findByText("Chat 5")).toBeInTheDocument();
    expect(getConversationMock).toHaveBeenCalledWith(5, expect.anything());
  });

  it("renders the not-found state with a link back when the server 404s", async () => {
    getConversationMock.mockRejectedValue(new Error("Conversation not found."));

    renderAt("/messages/9");

    expect(await screen.findByText("Conversation not found.")).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "Back to conversations" });
    expect(link).toHaveAttribute("href", "/");
  });

  it("renders the empty pane and does not fetch when there is no conversation id", async () => {
    renderAt("/messages");

    expect(await screen.findByText("Select a conversation to view messages")).toBeInTheDocument();
    expect(getConversationMock).not.toHaveBeenCalled();
  });

  it("does not fetch a non-numeric id and renders the not-found state", async () => {
    renderAt("/messages/abc");

    expect(await screen.findByText("Conversation not found.")).toBeInTheDocument();
    expect(getConversationMock).not.toHaveBeenCalled();
  });

  it("shows the stale location.state row immediately, then replaces it once the server answers", async () => {
    const stale = conv(7, "Stale Name");
    const fresh = conv(7, "Fresh Name");
    let resolveFetch: (c: Conversation) => void = () => {};
    getConversationMock.mockReturnValue(
      new Promise<Conversation>((resolve) => {
        resolveFetch = resolve;
      }),
    );

    renderAt("/messages/7", { conversation: stale });

    // The placeholder from location.state paints before the fetch settles.
    expect(await screen.findByText("Stale Name")).toBeInTheDocument();
    expect(getConversationMock).toHaveBeenCalledWith(7, expect.anything());

    resolveFetch(fresh);

    expect(await screen.findByText("Fresh Name")).toBeInTheDocument();
    expect(screen.queryByText("Stale Name")).not.toBeInTheDocument();
  });

  it("does not show a Move to trash error from one conversation on the next one opened", async () => {
    getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
    trashConversationMock.mockRejectedValue(new Error("Trash refused."));
    const user = userEvent.setup();

    renderAt("/messages/7");
    await screen.findByText("Chat 7");
    await user.click(screen.getByRole("button", { name: "More for this conversation" }));
    await user.click(screen.getByRole("menuitem", { name: "Move to trash" }));
    await screen.findByText("Trash refused.");

    await user.click(screen.getByRole("button", { name: "open 8" }));
    await screen.findByText("Chat 8");
    expect(screen.queryByText("Trash refused.")).not.toBeInTheDocument();
  });

  it("stays on the conversation now open when a Move to trash pressed on the last one succeeds", async () => {
    getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
    let finishTrash: () => void = () => {};
    trashConversationMock.mockReturnValue(
      new Promise<void>((resolve) => {
        finishTrash = resolve;
      }),
    );
    const user = userEvent.setup();

    renderAt("/messages/7");
    await screen.findByText("Chat 7");
    await user.click(screen.getByRole("button", { name: "More for this conversation" }));
    await user.click(screen.getByRole("menuitem", { name: "Move to trash" }));
    await waitFor(() => expect(trashConversationMock).toHaveBeenCalledWith(7, expect.anything()));

    await user.click(screen.getByRole("button", { name: "open 8" }));
    await screen.findByText("Chat 8");
    // Conversation 8 has nothing pending, so its Move to trash is live.
    await user.click(screen.getByRole("button", { name: "More for this conversation" }));
    expect(screen.getByRole("menuitem", { name: "Move to trash" })).not.toBeDisabled();
    await user.keyboard("{Escape}");

    finishTrash();
    await waitFor(() => expect(trashConversationMock.mock.results[0]?.type).toBe("return"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.getByText("Chat 8")).toBeInTheDocument();
    expect(screen.queryByText("Conversations list")).not.toBeInTheDocument();
  });

  it.each([
    ["a search", "?q=dentist", "dentist"],
    ["a tag page's filter", "?q=tag%3AWork+dentist", "tag:Work dentist"],
    ["a contact's conversations", "?q=with%3A%2342&f=with%3A%2342", "with:#42"],
  ])("keeps %s when another conversation in the list is opened", async (_name, search, query) => {
    getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
    const user = userEvent.setup();

    renderAt(`/messages/5${search}`);
    expect(screen.getByTestId("list-query").textContent).toBe(query);

    await user.click(screen.getByRole("button", { name: "Second result" }));
    expect(screen.getByTestId("location").textContent).toBe(`/messages/6${search}`);
    expect(screen.getByTestId("list-query").textContent).toBe(query);
  });

  describe("the Messages view", () => {
    it("lists messages for the search when the address asks for them", async () => {
      getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
      renderAt("/messages/5?q=photo&view=messages");
      expect(screen.getByTestId("message-list-query").textContent).toBe("photo");
      expect(screen.queryByTestId("list-query")).not.toBeInTheDocument();
      expect(screen.getByRole("radio", { name: "Messages", checked: true })).toBeInTheDocument();
    });

    it("opens a result's conversation at that message, keeping the list", async () => {
      getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
      const user = userEvent.setup();

      renderAt("/messages/5?q=photo&view=messages&msort=-date");
      await user.click(screen.getByRole("button", { name: "Message result" }));

      expect(screen.getByTestId("location").textContent).toBe(
        "/messages/6?q=photo&view=messages&msort=-date&at=77",
      );
      expect(screen.getByTestId("message-list-selected").textContent).toBe("77");
      await screen.findByText("Chat 6");
      // The panel reads the messages around the result, not the newest ones.
      await waitFor(() =>
        expect(listConversationMessagesMock).toHaveBeenCalledWith(
          6,
          expect.objectContaining({ around: 77 }),
          expect.anything(),
        ),
      );
    });

    it("drops the result's message when the switch goes back to Conversations and one is opened", async () => {
      getConversationMock.mockImplementation(async (id) => conv(id, `Chat ${id}`));
      const user = userEvent.setup();

      renderAt("/messages/5?q=photo&view=messages&at=77");
      await user.click(screen.getByRole("radio", { name: "Conversations" }));
      expect(screen.getByTestId("location").textContent).toBe("/messages/5?q=photo&at=77");

      await user.click(screen.getByRole("button", { name: "Second result" }));
      expect(screen.getByTestId("location").textContent).toBe("/messages/6?q=photo");
    });
  });
});
