/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ExportScreen from "./ExportScreen";

const invokePull = vi.hoisted(() => vi.fn());
const invokeFormat = vi.hoisted(() => vi.fn());
const invokeDeleteStaging = vi.hoisted(() => vi.fn());
const invokeCancel = vi.hoisted(() => vi.fn());
const resolveExportStagingDir = vi.hoisted(() => vi.fn());
const awaitTauriJob = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => true,
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    EXPORT_FORMATS: actual.EXPORT_FORMATS,
    invokePull: (...args: unknown[]) => invokePull(...args),
    invokeFormat: (...args: unknown[]) => invokeFormat(...args),
    invokeDeleteStaging: (...args: unknown[]) => invokeDeleteStaging(...args),
    invokeCancel: (...args: unknown[]) => invokeCancel(...args),
    awaitTauriJob: (...args: unknown[]) => awaitTauriJob(...args),
    onExtractEvents: vi.fn(async () => () => {}),
  };
});

vi.mock("../lib/system-settings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/system-settings")>();
  return {
    ...actual,
    resolveExportStagingDir: (...args: unknown[]) => resolveExportStagingDir(...args),
  };
});

vi.mock("../lib/api", () => ({
  getBaseUrl: () => "http://127.0.0.1:8080",
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ token: "test-token" }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  resolveExportStagingDir.mockResolvedValue(
    "/home/demo/message-vault/staging-export-260831-120000",
  );
  // The hook's `run` goes through awaitTauriJob: call the invoke and resolve.
  awaitTauriJob.mockImplementation(async (invokeFn: () => Promise<void>) => {
    await invokeFn();
    return { summary: "done" };
  });
});

/** The screen at `/export`, or at `/export?q=` when `query` is given. */
function renderScreen(query?: string) {
  const path = query === undefined ? "/export" : `/export?q=${encodeURIComponent(query)}`;
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ExportScreen />
    </MemoryRouter>,
  );
}

/** Fill the save folder and press Export. */
async function exportTo(folder: string) {
  const user = userEvent.setup();
  renderScreen();
  await user.type(screen.getByPlaceholderText("Choose folder…"), folder);
  await user.click(screen.getByRole("button", { name: "Export" }));
  return user;
}

/** Pick a format from the Format select, then press Export. */
async function exportAs(folder: string, formatLabel: string) {
  const user = userEvent.setup();
  renderScreen();
  await user.type(screen.getByPlaceholderText("Choose folder…"), folder);
  await user.click(screen.getByRole("button", { name: /Format/ }));
  await user.click(await screen.findByRole("option", { name: formatLabel }));
  await user.click(screen.getByRole("button", { name: "Export" }));
  return user;
}

describe("ExportScreen", () => {
  it("pulls straight into the chosen folder for JSON Lines", async () => {
    await exportTo("/home/demo/out");

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    // Everything is the scope the screen opens in without a query, and it
    // sends a blank query, which vault-pull reads as the whole account.
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: "/home/demo/out", query: "" });
    // JSONL is what pull already writes, so there is nothing to convert and
    // no staging folder to make or remove.
    expect(resolveExportStagingDir).not.toHaveBeenCalled();
    expect(invokeFormat).not.toHaveBeenCalled();
    expect(invokeDeleteStaging).not.toHaveBeenCalled();
  });

  it("pulls into staging and converts into the chosen folder for CSV", async () => {
    const staging = "/home/demo/message-vault/staging-export-260831-120000";
    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: staging });
    expect(invokeFormat.mock.calls[0][0]).toEqual({
      input_dir: staging,
      output_dir: "/home/demo/out",
      output_format: "csv",
    });
  });

  it("removes the staging folder once the conversion finishes", async () => {
    const staging = "/home/demo/message-vault/staging-export-260831-120000";
    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeDeleteStaging).toHaveBeenCalledWith({ staging_dir: staging }));
  });

  it("removes the staging folder even when the conversion fails", async () => {
    // Otherwise a failed export silently leaves a whole copy of the vault on
    // disk, in a folder the person never chose and will not think to look in.
    const staging = "/home/demo/message-vault/staging-export-260831-120000";
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await invokeFn();
      return { summary: "pulled" };
    });
    awaitTauriJob.mockImplementationOnce(async () => {
      throw new Error("unsupported output format");
    });

    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeDeleteStaging).toHaveBeenCalledWith({ staging_dir: staging }));
    expect(await screen.findByText("unsupported output format")).toBeTruthy();
  });

  it("ignores a second Export while one is already under way", async () => {
    // The desktop backend runs one job at a time (src-tauri/src/commands/jobs.rs).
    // Two exports started in the same second would also resolve to the same
    // staging folder, so the first cleanup would delete the second's files.
    let releasePull: () => void = () => {};
    const pullStarted = new Promise<void>((resolve) => {
      releasePull = resolve;
    });
    resolveExportStagingDir.mockImplementation(async () => {
      await pullStarted;
      return "/home/demo/message-vault/staging-export-260831-120000";
    });

    const user = userEvent.setup();
    renderScreen();
    await user.type(screen.getByPlaceholderText("Choose folder…"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: /Format/ }));
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));

    const exportButton = screen.getByRole("button", { name: "Export" });
    await user.click(exportButton);
    // Still resolving the staging path: the button must already be inert.
    await user.click(exportButton).catch(() => {});
    releasePull();

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokePull).toHaveBeenCalledTimes(1);
    expect(resolveExportStagingDir).toHaveBeenCalledTimes(1);
  });

  it("opens in Everything with no query box, and offers the box under Search", async () => {
    const user = userEvent.setup();
    renderScreen();
    expect(screen.getByRole("button", { name: /Scope/ })).toHaveTextContent("Everything");
    expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();

    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Search" }));
    expect(screen.getByRole("textbox", { name: "Search" })).toBeTruthy();
  });

  it("sends the query typed under Search", async () => {
    const user = userEvent.setup();
    renderScreen();
    await user.type(screen.getByPlaceholderText("Choose folder…"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Search" }));
    await user.type(screen.getByRole("textbox", { name: "Search" }), " in:#19,#22 ");
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    expect(invokePull.mock.calls[0][0]).toMatchObject({ query: "in:#19,#22" });
  });

  it("opens in Search with the query it was given, and sends it", async () => {
    // LeftPanel hands over the conversation list's query as `?q=`, so the
    // person sees what "the current view" means before exporting it.
    const user = userEvent.setup();
    renderScreen("from:me tag:Work");
    expect(screen.getByRole("button", { name: /Scope/ })).toHaveTextContent("Search");
    expect(screen.getByRole("textbox", { name: "Search" })).toHaveValue("from:me tag:Work");

    await user.type(screen.getByPlaceholderText("Choose folder…"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    expect(invokePull.mock.calls[0][0]).toMatchObject({ query: "from:me tag:Work" });
  });

  it("will not export a Search scope with a blank query", async () => {
    // vault-pull reads a blank query as the whole account, which is not what
    // someone who chose Search and left the box empty asked for.
    const user = userEvent.setup();
    renderScreen("from:me");
    await user.type(screen.getByPlaceholderText("Choose folder…"), "/home/demo/out");
    await user.clear(screen.getByRole("textbox", { name: "Search" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Everything" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
  });

  it("reports the failure rather than claiming the export finished", async () => {
    awaitTauriJob.mockImplementation(async () => {
      throw new Error("vault key is required");
    });

    await exportTo("/home/demo/out");

    expect(await screen.findByText("vault key is required")).toBeTruthy();
    expect(screen.queryByText(/Export complete/)).toBeNull();
  });
});
