/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getConversation, listConversationMessages } from "../lib/serverApi";
import type { Conversation } from "../lib/types";
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
}));

const getConversationMock = vi.mocked(getConversation);
const listConversationMessagesMock = vi.mocked(listConversationMessages);

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

function renderAt(path: string, state?: unknown) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={[{ pathname: path, state }]}>
        <RightToolbarProvider>
          <Routes>
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
});
