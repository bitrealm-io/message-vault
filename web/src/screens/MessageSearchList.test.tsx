/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageSearchSort } from "../lib/messageSearchSort";
import { listMessages } from "../lib/serverApi";
import { mockedAuth, Providers } from "../test/providers";
import MessageSearchList from "./MessageSearchList";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  listMessages: vi.fn(),
}));

const listMessagesMock = vi.mocked(listMessages);

// jsdom has no ResizeObserver; VirtualList observes its scroll container on mount.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  listMessagesMock.mockReset();
  listMessagesMock.mockResolvedValue({ items: [], total: 12408, limit: 40, offset: 0 });
});

afterEach(() => {
  vi.unstubAllGlobals();
  cleanup();
});

function renderList(query: string, sortPick: MessageSearchSort | null = null) {
  const onSortPick = vi.fn();
  render(
    <Providers>
      <MessageSearchList
        query={query}
        sortPick={sortPick}
        onSortPick={onSortPick}
        selectedId={null}
        onSelect={() => {}}
      />
    </Providers>,
  );
  return { onSortPick };
}

/** The labels of the sort menu's choices, Sort By then Order, once it is open. */
async function sortChoices(): Promise<string[]> {
  await userEvent.setup().click(screen.getByRole("button", { name: /^Sort messages by/ }));
  const menu = screen.getByRole("menu", { name: "Sort messages" });
  return [...menu.querySelectorAll('[role="menuitemradio"]')].map((item) => item.textContent ?? "");
}

describe("MessageSearchList", () => {
  it("asks for a search, and lists nothing, when the search is empty", () => {
    renderList("  ");
    expect(screen.getByText("Type a search above to list the messages it matches.")).toBeVisible();
    expect(listMessagesMock).not.toHaveBeenCalled();
  });

  it("shows the total, and sorts by relevance by default for a free-text word", async () => {
    renderList("from:Alice photo");
    expect(await screen.findByText("12,408 messages")).toBeVisible();
    expect(listMessagesMock).toHaveBeenCalledWith(
      { q: "from:Alice photo", sort: "relevance", limit: 40, offset: 0 },
      expect.anything(),
    );
    expect(screen.getByRole("button", { name: "Sort messages by Relevance" })).toBeVisible();
    expect(await sortChoices()).toEqual(["Relevance", "Date"]);
  });

  it("offers only Date, newest first, when the search has no free-text word", async () => {
    renderList("from:Alice date:2024");
    await waitFor(() =>
      expect(listMessagesMock).toHaveBeenCalledWith(
        expect.objectContaining({ q: "from:Alice date:2024", sort: "-date" }),
        expect.anything(),
      ),
    );
    expect(
      screen.getByRole("button", { name: "Sort messages by Date, Newest first" }),
    ).toBeVisible();
    expect(await sortChoices()).toEqual(["Date", "Oldest first", "Newest first"]);
  });

  it("keeps a picked date order, and hands a new pick to the caller", async () => {
    const { onSortPick } = renderList("photo", { sort: "date", order: "asc" });
    await waitFor(() =>
      expect(listMessagesMock).toHaveBeenCalledWith(
        expect.objectContaining({ sort: "date" }),
        expect.anything(),
      ),
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sort messages by Date, Oldest first" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Relevance" }));
    expect(onSortPick).toHaveBeenCalledWith({ sort: "relevance", order: "asc" });
  });
});
