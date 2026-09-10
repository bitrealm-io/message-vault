/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, VaultProviders } from "../test/vaultProviders";
import LeftPanel from "./LeftPanel";

const profileState = vi.hoisted(() => ({
  profile: null as object | null,
}));
const tauriState = vi.hoisted(() => ({ isTauri: false }));
const savedSearchState = vi.hoisted(() => ({
  savedSearches: [] as { id: number; name: string; query: string; kind: string }[],
}));

vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: profileState.profile }),
}));

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/useContactGroups", () => ({
  useContactGroups: () => ({ groups: [] }),
}));

vi.mock("../lib/useMessageTags", () => ({
  useMessageTags: () => ({ tags: [] }),
}));

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

const importAttentionState = vi.hoisted(() => ({
  attention: null as "waiting" | "failed" | null,
}));

vi.mock("../screens/import/useImportAttention", () => ({
  useImportAttention: () => importAttentionState.attention,
}));

vi.mock("../lib/savedSearches", () => ({
  useSavedSearches: () => ({ savedSearches: savedSearchState.savedSearches, loading: false }),
  useSavedSearchActions: () => ({
    create: vi.fn(),
    update: vi.fn(),
    remove: vi.fn(),
  }),
}));

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  localStorage.clear();
  profileState.profile = null;
  tauriState.isTauri = false;
  savedSearchState.savedSearches = [];
  importAttentionState.attention = null;
});

function renderPanel(initialEntries?: string[]) {
  return render(
    <VaultProviders>
      <MemoryRouter initialEntries={initialEntries}>
        <LeftPanel onSearchChange={() => {}} />
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("LeftPanel", () => {
  it("puts browse icons in the shared 15px leading slot", () => {
    renderPanel();
    const messages = screen.getByRole("button", { name: "Messages" });
    expect(messages.querySelector('[class*="size-[15px]"]')).not.toBeNull();
    expect(messages.className).not.toContain("pl-[calc(15px+0.5rem)]");
  });

  it("lines empty saved-search copy up with the heading title slot", () => {
    renderPanel();
    const empty = screen.getByText("No saved searches");
    const row = empty.parentElement;
    expect(row?.querySelector('[class*="size-[15px]"]')).not.toBeNull();
    expect(row?.className).not.toContain("pl-[calc(15px+0.5rem)]");
  });

  it("indents named saved searches like nested group rows", () => {
    savedSearchState.savedSearches = [
      { id: 1, name: "From Alice", query: "from:alice", kind: "manual" },
    ];
    renderPanel();
    const alice = screen.getByRole("button", { name: "From Alice" });
    expect(alice.className).toContain("pl-[calc(15px+0.5rem)]");
    expect(alice.className).toContain("self-stretch");
    expect(alice.querySelector('[class*="size-[15px]"]')).not.toBeNull();
  });

  it("closes the saved-search options menu on Escape", async () => {
    savedSearchState.savedSearches = [
      { id: 1, name: "From Alice", query: "from:alice", kind: "manual" },
    ];
    const user = userEvent.setup();
    renderPanel();
    await user.click(screen.getByRole("button", { name: "Saved search options for From Alice" }));
    expect(screen.getByRole("menuitem", { name: "Rename…" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem", { name: "Rename…" })).toBeNull();
  });

  describe("desktop Import/Export Messages section", () => {
    beforeEach(() => {
      tauriState.isTauri = true;
      profileState.profile = {};
    });

    it("shows a left-chevron Messages heading with aria-expanded", () => {
      renderPanel();
      const heading = screen.getByRole("button", { name: "Messages", expanded: true });
      expect(heading.getAttribute("aria-expanded")).toBe("true");
      expect(heading.querySelector('[class*="size-[15px]"]')).not.toBeNull();
      expect(heading.className).toContain("col-span-2");
      expect(heading.querySelector('[class*="motion-reduce:transition-none"]')).not.toBeNull();
    });

    it("highlights the Messages heading when Import is the current route", () => {
      renderPanel(["/import"]);
      const heading = screen.getByRole("button", { name: "Messages", expanded: true });
      expect(heading.className).toMatch(/bg-hover/);
    });

    it("indents Import and Export like nested group rows", () => {
      renderPanel();
      const importBtn = screen.getByRole("button", { name: "Import" });
      const exportBtn = screen.getByRole("button", { name: "Export" });
      for (const btn of [importBtn, exportBtn]) {
        const nested = btn.querySelector('[class*="pl-[calc(15px+0.5rem)]"]');
        expect(nested).not.toBeNull();
        expect(nested?.className).toContain("self-stretch");
        expect(nested?.querySelector('[class*="size-[15px]"]')).not.toBeNull();
      }
    });

    it("marks Import when a run is waiting for the person, and when the last one failed", () => {
      importAttentionState.attention = "waiting";
      const { unmount } = renderPanel();
      expect(screen.getByRole("button", { name: /Import/ })).toHaveTextContent("Waiting");
      expect(screen.getByTitle("An import is waiting for your approval")).toBeTruthy();
      unmount();

      importAttentionState.attention = "failed";
      renderPanel();
      expect(screen.getByRole("button", { name: /Import/ })).toHaveTextContent("Failed");
    });

    it("carries no badge while nothing needs the person", () => {
      renderPanel();
      expect(screen.getByRole("button", { name: "Import" })).toBeTruthy();
      expect(screen.queryByText("Waiting")).toBeNull();
      expect(screen.queryByText("Failed")).toBeNull();
    });

    it("hides Import and Export when the Messages heading collapses", async () => {
      const user = userEvent.setup();
      renderPanel();
      expect(screen.getByRole("button", { name: "Import" })).toBeTruthy();
      expect(screen.getByRole("button", { name: "Export" })).toBeTruthy();

      await user.click(screen.getByRole("button", { name: "Messages", expanded: true }));
      expect(screen.queryByRole("button", { name: "Import" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Export" })).toBeNull();
      expect(screen.getByRole("button", { name: "Messages", expanded: false })).toBeTruthy();
    });

    it("keeps browse Messages without nested padding", () => {
      renderPanel();
      const browse = screen
        .getAllByRole("button", { name: "Messages" })
        .find((btn) => btn.getAttribute("aria-expanded") == null);
      expect(browse).toBeTruthy();
      expect(browse?.className).not.toContain("pl-[calc(15px+0.5rem)]");
      expect(browse?.querySelector('[class*="size-[15px]"]')).not.toBeNull();
    });
  });
});
