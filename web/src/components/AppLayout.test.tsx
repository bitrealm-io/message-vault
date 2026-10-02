/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import AppLayout from "./AppLayout";

// The lists, the header and the drawers fetch their own data; this file is
// about what the layout does to the URL, so they stand in as nothing.
vi.mock("../screens/ContactList", () => ({ default: () => null }));
vi.mock("../screens/ConversationList", () => ({ default: () => null }));
vi.mock("./AppHeader", () => ({ default: () => null }));
vi.mock("./ContactDrawer", () => ({ default: () => null }));
vi.mock("./CheckedContactsPanel", () => ({ default: () => null }));

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));
vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: null }),
}));
vi.mock("../lib/useContactGroups", () => ({
  useContactGroups: () => ({ groups: [] }),
}));
vi.mock("../lib/useMessageTags", () => ({
  useMessageTags: () => ({ tags: [] }),
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
});
