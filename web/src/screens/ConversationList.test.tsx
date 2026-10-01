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

import { cleanup, render } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RightToolbarProvider } from "../components/RightToolbarContext";
import { mockedAuth, Providers } from "../test/providers";
import ConversationList from "./ConversationList";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/serverApi", () => ({
  // messageTags.ts pulls slug helpers from contactGroups.ts, whose module-level
  // `createNameCollection` call needs these even though this test never uses them.
  listContactGroups: vi.fn().mockResolvedValue({ items: [] }),
  createContactGroup: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
  listMessageTags: vi.fn().mockResolvedValue({
    items: [
      { id: 1, name: "Holiday" },
      { id: 2, name: "Receipts" },
    ],
  }),
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
});
