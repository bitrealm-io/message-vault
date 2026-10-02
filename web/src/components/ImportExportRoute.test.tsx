/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { importRunStore, initialImportRunState } from "../screens/import/importRunStore";
import ImportExportRoute from "./ImportExportRoute";

type Flags = { can_import: boolean; can_export: boolean; is_demo: boolean };

const state = vi.hoisted(() => ({
  isTauri: true,
  profile: null as Flags | null,
  loading: false,
  logout: vi.fn(async () => {}),
}));

vi.mock("../lib/tauri-check", () => ({ isTauri: () => state.isTauri }));
vi.mock("../lib/useAccountProfile", () => ({
  useAccountProfile: () => ({ profile: state.profile, loading: state.loading, error: "" }),
}));
vi.mock("../lib/auth", () => ({ useAuth: () => ({ logout: state.logout }) }));

afterEach(() => {
  cleanup();
  state.isTauri = true;
  state.profile = null;
  state.loading = false;
  state.logout.mockClear();
  importRunStore.reset(initialImportRunState([]));
});

const allowed: Flags = { can_import: true, can_export: true, is_demo: false };

function renderRoute(feature: "import" | "export") {
  return render(
    <MemoryRouter initialEntries={[`/${feature}`]}>
      <Routes>
        <Route path="/" element={<div>Messages</div>} />
        <Route
          path={`/${feature}`}
          element={
            <ImportExportRoute feature={feature}>
              <div>the form</div>
            </ImportExportRoute>
          }
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("ImportExportRoute", () => {
  it("shows the form to an account that holds the permission", async () => {
    state.profile = allowed;
    renderRoute("import");
    expect(await screen.findByText("the form")).toBeTruthy();
  });

  it("goes to Messages in the browser", () => {
    state.isTauri = false;
    state.profile = allowed;
    renderRoute("import");
    expect(screen.getByText("Messages")).toBeTruthy();
    expect(screen.queryByText("the form")).toBeNull();
  });

  it("shows nothing while the profile loads", () => {
    state.loading = true;
    const { container } = renderRoute("import");
    expect(container.textContent).toBe("");
  });

  it("explains, in place of the Import form, that the Owner has not allowed importing", () => {
    state.profile = { ...allowed, can_import: false };
    renderRoute("import");
    expect(screen.getByRole("heading", { name: "Import" })).toBeTruthy();
    expect(screen.getByText(/The Owner has not allowed this account to import\./)).toBeTruthy();
    expect(screen.queryByText("the form")).toBeNull();
    expect(screen.queryByRole("button", { name: "Log out" })).toBeNull();
  });

  it("explains, in place of the Export form, that the Owner has not allowed exporting", () => {
    state.profile = { ...allowed, can_export: false };
    renderRoute("export");
    expect(screen.getByRole("heading", { name: "Export" })).toBeTruthy();
    expect(screen.getByText(/The Owner has not allowed this account to export\./)).toBeTruthy();
    expect(screen.queryByText("the form")).toBeNull();
  });

  it("keeps Export open to an account that may export but not import", async () => {
    state.profile = { ...allowed, can_import: false };
    renderRoute("export");
    expect(await screen.findByText("the form")).toBeTruthy();
  });

  it("tells the Demo Account that importing needs another account, and logs out from there", async () => {
    const user = userEvent.setup();
    state.profile = { can_import: false, can_export: true, is_demo: true };
    renderRoute("import");
    expect(screen.getByText(/The Demo Account can't import\./)).toBeTruthy();
    expect(screen.queryByText("the form")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Log out" }));
    expect(state.logout).toHaveBeenCalledTimes(1);
  });

  it("gives the Demo Account the ordinary message on Export", () => {
    state.profile = { can_import: false, can_export: false, is_demo: true };
    renderRoute("export");
    expect(screen.getByText(/The Owner has not allowed this account to export\./)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Log out" })).toBeNull();
  });

  it("keeps an Import Run already in progress on screen, with the reason it will fail", async () => {
    state.profile = { ...allowed, can_import: false };
    importRunStore.set({ phase: "running", running: true });
    renderRoute("import");
    expect(await screen.findByText("the form")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toMatch(
      /The Owner has turned Import off for this account\./,
    );
    expect(screen.queryByRole("heading", { name: "Import" })).toBeNull();
  });

  it("shows the message once that run is left", async () => {
    state.profile = { ...allowed, can_import: false };
    importRunStore.set({ phase: "done" });
    renderRoute("import");
    expect(await screen.findByText("the form")).toBeTruthy();
    act(() => importRunStore.set({ phase: "form" }));
    expect(screen.getByText(/The Owner has not allowed this account to import\./)).toBeTruthy();
    expect(screen.queryByText("the form")).toBeNull();
  });

  it("does not let a run in progress open Export, for an account that may export", async () => {
    state.profile = allowed;
    importRunStore.set({ phase: "running", running: true });
    renderRoute("export");
    // Let the Suspense boundary settle before asserting.
    await act(async () => {});
    expect(screen.queryByText("the form")).toBeNull();
    expect(screen.getByRole("status").textContent).toMatch(
      /An Import Run is running\. Export can start once it ends\./,
    );
  });
});
