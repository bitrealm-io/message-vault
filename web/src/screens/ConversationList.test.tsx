/** @vitest-environment jsdom */

/**
 * Regression test for issue #295: on every render `ConversationList` handed
 * a freshly built `<TagsMenu>` to `RightToolbarContext`, whose provider
 * re-renders its whole subtree — `ConversationList` included — on every
 * write. The effect's dependencies (`targetConversations`, `tagChecks`, and
 * the `applyMembership` callback they feed) were rebuilt from a `conversations`
 * array that `useRoutePagedList` reallocated on every call, so the effect's
 * dependency array was never equal to the last render's, the effect fired
 * again after every one of those forced re-renders, and the two fed each
 * other into "Maximum update depth exceeded" before a person touched
 * anything.
 */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RightPane from "../components/RightPane";
import { RightToolbarProvider } from "../components/RightToolbarContext";
import { listConversations, updateMessageTagMembers } from "../lib/serverApi";
import type { Conversation } from "../lib/types";
import { mockedAuth, Providers } from "../test/providers";
import ConversationList from "./ConversationList";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", () => ({
  // messageTags.ts pulls slug helpers from contactGroups.ts, whose module-level
  // `createNameCollection` call needs these even though this test never uses them.
  listContactGroups: vi.fn().mockResolvedValue([]),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
  listMessageTags: vi.fn().mockResolvedValue([
    { id: 1, name: "Holiday" },
    { id: 2, name: "Receipts" },
  ]),
  createMessageTag: vi.fn(),
  updateMessageTag: vi.fn(),
  deleteMessageTag: vi.fn(),
  updateMessageTagMembers: vi.fn(),
  listConversations: vi.fn().mockResolvedValue({
    items: [
      { id: 1, display_name: "Alice", tags: ["Holiday"] },
      { id: 2, display_name: "Bob", tags: [] },
    ],
    total: 2,
    limit: 40,
    offset: 0,
  }),
}));

// jsdom has no ResizeObserver; VirtualList observes its scroll container on mount.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  consoleErrorSpy.mockRestore();
  vi.unstubAllGlobals();
  cleanup();
});

/** Every logged message, joined, so one assertion covers every call shape. */
function loggedErrors(): string {
  return consoleErrorSpy.mock.calls
    .map((call: unknown[]) =>
      call.map((arg) => (typeof arg === "string" ? arg : String(arg))).join(" "),
    )
    .join("\n");
}

describe("ConversationList", () => {
  it("registers the tag menu into the right toolbar without looping", async () => {
    render(
      <Providers>
        <MemoryRouter>
          <RightToolbarProvider>
            <ConversationList selectedId={null} onSelect={() => {}} query="" />
          </RightToolbarProvider>
        </MemoryRouter>
      </Providers>,
    );

    // Give the effect and any resulting re-renders a chance to settle. If the
    // loop is present, React's nested-update guard trips well inside this
    // window rather than the test just quietly hanging.
    await new Promise((resolve) => setTimeout(resolve, 100));

    const logged = loggedErrors();
    expect(logged).not.toMatch(/Maximum update depth/);
    expect(logged).not.toMatch(/Should have a queue/);
  });

  describe("Select all", () => {
    // jsdom lays nothing out, so the virtual list would draw no rows. A
    // 400-pixel viewport is all TanStack Virtual reads to place them.
    let restoreLayout: (() => void)[] = [];
    beforeEach(() => {
      const viewport = 400;
      restoreLayout = [
        vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(viewport),
        vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300),
        vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(viewport),
        vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
          top: 0,
          bottom: viewport,
          left: 0,
          right: 300,
          width: 300,
          height: viewport,
        } as DOMRect),
      ].map((spy) => () => spy.mockRestore());
    });
    afterEach(() => {
      for (const restore of restoreLayout) restore();
    });

    function chat(id: number): Conversation {
      return {
        id,
        participants: [],
        message_count: 1,
        last_message_at: "",
        date_range_start: null,
        date_range_end: null,
        service: "sms",
        is_group: false,
        label: `Chat ${id}`,
        tags: [],
      };
    }

    /** The server's paging over `count` conversations. */
    function serveConversations(count: number) {
      const all = Array.from({ length: count }, (_, i) => chat(i + 1));
      vi.mocked(listConversations).mockImplementation(async ({ limit = 40, offset = 0 }) => ({
        items: all.slice(offset, offset + limit),
        total: all.length,
        limit,
        offset,
      }));
    }

    function renderList() {
      return render(
        <Providers>
          <MemoryRouter>
            <RightToolbarProvider>
              <RightPane>
                <ConversationList selectedId={null} onSelect={() => {}} query="" />
              </RightPane>
            </RightToolbarProvider>
          </MemoryRouter>
        </Providers>,
      );
    }

    it("does not read as all when more conversations are on the server", async () => {
      vi.mocked(listConversations).mockResolvedValue({
        items: [chat(1), chat(2)],
        total: 3000,
        limit: 40,
        offset: 0,
      });
      renderList();
      const user = userEvent.setup({ delay: null });
      await user.click(await screen.findByRole("checkbox", { name: "Select Chat 1" }));
      await user.click(screen.getByRole("checkbox", { name: "Select Chat 2" }));

      // Two of 3,000 are ticked.
      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      expect(box).not.toBeChecked();
    });

    it("tags every conversation the list holds, not only the loaded page", async () => {
      serveConversations(120);
      vi.mocked(updateMessageTagMembers).mockResolvedValue({ added: 120, removed: 0 });
      renderList();
      const user = userEvent.setup({ delay: null });
      await screen.findByRole("checkbox", { name: "Select Chat 1" });

      const box = screen.getByRole("checkbox", { name: "Select all conversations" });
      await user.click(box);
      await waitFor(() => expect(box).toBeChecked());

      await user.click(screen.getByRole("button", { name: "Tags" }));
      await user.click(await screen.findByRole("checkbox", { name: "Holiday" }));

      await waitFor(() => expect(vi.mocked(updateMessageTagMembers)).toHaveBeenCalled());
      const [id, body] = vi.mocked(updateMessageTagMembers).mock.calls[0] ?? [];
      expect(id).toBe(1);
      expect([...(body?.add ?? [])].sort((a, b) => a - b)).toEqual(
        Array.from({ length: 120 }, (_, i) => i + 1),
      );
    });
  });
});
