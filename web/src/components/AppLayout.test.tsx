/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import AppLayout from "./AppLayout";

// The lists, the header and the drawers fetch their own data; this file is
// about what the layout does to the URL, so they stand in as nothing, except
// the two lists. Each says which filter it was given, so a test can tell a
// list filtered to a set from a list of everything, and the conversation list
// offers one row to click.
vi.mock("../screens/ContactList", () => ({
  default: ({ groupFilter }: { groupFilter: string | null }) => (
    <div data-testid="contact-list">{`group filter: ${groupFilter ?? "(none)"}`}</div>
  ),
}));
vi.mock("../screens/ConversationList", () => ({
  default: ({ query, onSelect }: { query: string; onSelect: (c: { id: number }) => void }) => (
    <div>
      <div data-testid="conversation-list">{`query: ${query}`}</div>
      <button type="button" onClick={() => onSelect({ id: 6 })}>
        First result
      </button>
    </div>
  ),
}));
// The header stands in as one button that searches for "ada".
vi.mock("./AppHeader", () => ({
  default: ({ onSearch }: { onSearch: (q: string) => void }) => (
    <button type="button" onClick={() => onSearch("ada")}>
      Search for ada
    </button>
  ),
}));
vi.mock("./ContactDrawer", () => ({ default: () => null }));
vi.mock("./CheckedContactsPanel", () => ({ default: () => null }));

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));
vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: null }),
}));
const sets = vi.hoisted(() => ({
  groups: [] as string[],
  groupsLoading: false,
  tags: [] as string[],
  tagsLoading: false,
}));
vi.mock("../lib/useContactGroups", () => ({
  useContactGroups: () => ({ groups: sets.groups, loading: sets.groupsLoading }),
}));
vi.mock("../lib/useMessageTags", () => ({
  useMessageTags: () => ({ tags: sets.tags, loading: sets.tagsLoading }),
}));
vi.mock("../lib/tauri-check", () => ({ isTauri: () => false }));
vi.mock("../screens/import/useImportAttention", () => ({
  useImportAttention: () => null,
}));
vi.mock("../lib/savedSearches", () => ({
  useSavedSearches: () => ({
    savedSearches: [{ id: 1, name: "Groups", query: "kind:group", kind: "conversations" }],
    loading: false,
  }),
  useSavedSearchActions: () => ({
    create: vi.fn(),
    update: vi.fn(),
    remove: vi.fn(),
    pending: false,
    error: null,
  }),
}));

afterEach(() => {
  cleanup();
  sets.groups = [];
  sets.groupsLoading = false;
  sets.tags = [];
  sets.tagsLoading = false;
});

/** Where the router is now, and a Back button. */
// biome-ignore lint/style/useComponentExportOnlyModules: local test harness only
function HistoryProbe() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <output data-testid="location">{location.pathname + location.search}</output>
      <button type="button" onClick={() => navigate(-1)}>
        Back
      </button>
    </>
  );
}

function renderLayout(entry: string) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route element={<AppLayout />}>
            <Route path="*" element={null} />
          </Route>
        </Routes>
        <HistoryProbe />
      </MemoryRouter>
    </Providers>,
  );
}

describe("AppLayout", () => {
  it.each(["/contacts?cq=alice", "/trash?tq=bob&tsel=7"])(
    "leaves %s as it was when a Saved Search is opened from it",
    async (entry) => {
      const user = userEvent.setup();
      renderLayout(entry);

      await user.click(screen.getByRole("button", { name: "Groups" }));
      expect(screen.getByTestId("location").textContent).toBe("/?q=kind%3Agroup");

      await user.click(screen.getByRole("button", { name: "Back" }));
      expect(screen.getByTestId("location").textContent).toBe(entry);
    },
  );

  it.each([
    ["a search", "?q=dentist"],
    ["a contact's conversations", "?q=with%3A%2342&f=with%3A%2342"],
    ["the Messages list's picked sort", "?q=dentist&msort=date"],
  ])("keeps %s when a conversation in the list is opened", async (_name, search) => {
    const user = userEvent.setup();
    renderLayout(`/${search}`);

    await user.click(screen.getByRole("button", { name: "First result" }));
    expect(screen.getByTestId("location").textContent).toBe(`/messages/6${search}`);
  });
});

describe("AppLayout on a Contact Group or Message Tag page", () => {
  it("filters the contacts to the group whose whole name is in the link", () => {
    sets.groups = ["A&B", "A B", "家族"];
    renderLayout(`/group/${encodeURIComponent("A B")}`);
    expect(screen.getByTestId("contact-list")).toHaveTextContent("group filter: A B");
  });

  it("says a tag that does not exist is not found, and lists nothing", () => {
    sets.tags = ["Holiday"];
    renderLayout("/tag/Old");
    expect(screen.queryByTestId("conversation-list")).toBeNull();
    expect(screen.getByText("There is no Message Tag named Old.")).toBeTruthy();
  });

  it("says a group that does not exist is not found, and lists nothing", () => {
    sets.groups = ["Family"];
    renderLayout("/group/Old");
    expect(screen.queryByTestId("contact-list")).toBeNull();
    expect(screen.getByText("There is no Contact Group named Old.")).toBeTruthy();
  });

  it("shows that the groups are loading, and lists nothing until they have", () => {
    sets.groupsLoading = true;
    renderLayout("/group/Family");
    expect(screen.queryByTestId("contact-list")).toBeNull();
    expect(screen.getByText("Loading Contact Groups…")).toBeTruthy();
  });

  it("shows that the tags are loading, and lists nothing until they have", () => {
    sets.tagsLoading = true;
    renderLayout("/tag/Holiday");
    expect(screen.queryByTestId("conversation-list")).toBeNull();
    expect(screen.getByText("Loading Message Tags…")).toBeTruthy();
  });

  it("keeps a group whose name holds a question mark when the list is searched", async () => {
    sets.groups = ["Why?"];
    const user = userEvent.setup();
    renderLayout(`/group/${encodeURIComponent("Why?")}`);

    await user.click(screen.getByRole("button", { name: "Search for ada" }));
    expect(screen.getByTestId("location").textContent).toBe("/group/Why%3F?cq=ada");
  });

  it("keeps a tag whose name holds a number sign when the list is searched", async () => {
    sets.tags = ["#1"];
    const user = userEvent.setup();
    renderLayout(`/tag/${encodeURIComponent("#1")}`);

    await user.click(screen.getByRole("button", { name: "Search for ada" }));
    expect(screen.getByTestId("location").textContent).toBe("/tag/%231?q=ada");
  });
});
