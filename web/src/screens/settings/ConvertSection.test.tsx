/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { holdDesktopJob } from "../../lib/desktopJob";
import { ConvertSection } from "./ConvertSection";

const tauriState = vi.hoisted(() => ({ isTauri: true }));
const invokeFormat = vi.hoisted(() => vi.fn());
const invokeCancel = vi.hoisted(() => vi.fn());
const awaitTauriJob = vi.hoisted(() => vi.fn());

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/tauri")>();
  return {
    EXPORT_FORMATS: actual.EXPORT_FORMATS,
    invokeFormat: (...args: unknown[]) => invokeFormat(...args),
    invokeCancel: (...args: unknown[]) => invokeCancel(...args),
    // The job's name comes first; the mocks below take what follows it.
    awaitTauriJob: (_job: string, ...args: unknown[]) => awaitTauriJob(...args),
    onExtractEvents: vi.fn(async () => () => {}),
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  tauriState.isTauri = true;
  // The hook's `run` goes through awaitTauriJob: call the invoke and resolve.
  awaitTauriJob.mockImplementation(async (invokeFn: () => Promise<void>) => {
    await invokeFn();
    return { summary: "Format conversion complete." };
  });
});

const convertButton = () => screen.getByRole("button", { name: "Convert" });

/** Fill both folders; the format stays at its default unless `formatLabel` is given. */
async function fillFolders(input: string, output: string, formatLabel?: string) {
  const user = userEvent.setup();
  render(<ConvertSection />);
  await user.type(screen.getByLabelText("Input folder"), input);
  await user.type(screen.getByLabelText("Output folder"), output);
  if (formatLabel) {
    await user.click(screen.getByRole("button", { name: /Output format/ }));
    await user.click(await screen.findByRole("option", { name: formatLabel }));
  }
  return user;
}

describe("ConvertSection", () => {
  it("does not start while another desktop job runs, and names that job", async () => {
    const release = holdDesktopJob("Export");
    try {
      await fillFolders("/home/demo/export-json", "/home/demo/export-csv");
      expect(convertButton()).toBeDisabled();
      expect(screen.getByRole("status").textContent).toBe(
        "An export is running. Convert can start once it ends.",
      );
    } finally {
      release();
    }
    await waitFor(() => expect(convertButton()).toBeEnabled());
  });

  it("shows the desktop-only stub when not in Tauri", () => {
    tauriState.isTauri = false;
    render(<ConvertSection />);
    expect(screen.getByText(/available in the desktop app/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Convert" })).toBeNull();
  });

  it("keeps Convert disabled until both folders are filled", async () => {
    const user = userEvent.setup();
    render(<ConvertSection />);
    expect(convertButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Input folder"), "/home/demo/export-json");
    expect(convertButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Output folder"), "/home/demo/export-csv");
    expect(convertButton()).toBeEnabled();
  });

  it("refuses the same folder for input and output and says so", async () => {
    // message-reexport would reject this anyway, but only after the job had
    // started; the screen states the rule before the button is live.
    const user = await fillFolders("/home/demo/export", "/home/demo/export/");

    expect(screen.getByRole("alert")).toHaveTextContent(/two folders must differ/);
    expect(convertButton()).toBeDisabled();
    await user.click(convertButton()).catch(() => {});
    expect(invokeFormat).not.toHaveBeenCalled();
  });

  it("clears the folder message once the output folder changes", async () => {
    const user = await fillFolders("/home/demo/export", "/home/demo/export");
    expect(screen.getByRole("alert")).toBeTruthy();

    await user.type(screen.getByLabelText("Output folder"), "-csv");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(convertButton()).toBeEnabled();
  });

  it("runs the format command with both folders and the chosen output format", async () => {
    const user = await fillFolders("/home/demo/export-json", "/home/demo/export-csv", "CSV (.csv)");
    await user.click(convertButton());

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeFormat.mock.calls[0][0]).toEqual({
      input_dir: "/home/demo/export-json",
      output_dir: "/home/demo/export-csv",
      output_format: "csv",
    });
    expect(await screen.findByText(/Conversion complete\. CSV \(\.csv\) written to/)).toBeTruthy();
  });

  it("defaults the output format to JSON Lines", async () => {
    const user = await fillFolders("/home/demo/export-xml", "/home/demo/export-jsonl");
    await user.click(convertButton());

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeFormat.mock.calls[0][0]).toMatchObject({ output_format: "jsonl" });
  });

  it("reports the failure rather than claiming the conversion finished", async () => {
    awaitTauriJob.mockImplementation(async () => {
      throw new Error("input and output directories must be different");
    });
    const user = await fillFolders("/home/demo/link-to-export", "/home/demo/export");
    await user.click(convertButton());

    expect(await screen.findByText("input and output directories must be different")).toBeTruthy();
    expect(screen.queryByText(/Conversion complete/)).toBeNull();
  });

  it("names the folder and format the conversion wrote to after the form changes", async () => {
    const user = await fillFolders("/home/demo/export-json", "/a", "CSV (.csv)");
    await user.click(convertButton());
    await screen.findByText(/Conversion complete/);

    const field = screen.getByLabelText("Output folder");
    await user.clear(field);
    await user.type(field, "/b");
    await user.click(screen.getByRole("button", { name: /Output format/ }));
    await user.click(await screen.findByRole("option", { name: "JSON Lines (.jsonl)" }));

    expect(screen.getByText(/Conversion complete/)).toHaveTextContent(
      "Conversion complete. CSV (.csv) written to /a.",
    );
  });

  it("locks both folder fields while a conversion runs", async () => {
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await held;
      await invokeFn();
      return { summary: "Format conversion complete." };
    });

    const user = await fillFolders("/home/demo/export-json", "/a");
    await user.click(convertButton());
    expect(screen.getByLabelText("Input folder")).toBeDisabled();
    expect(screen.getByLabelText("Output folder")).toBeDisabled();
    release();
    await screen.findByText(/Conversion complete/);
    expect(screen.getByLabelText("Output folder")).toBeEnabled();
  });
});
