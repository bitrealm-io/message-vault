/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { APP_BUILD } from "../../lib/build";
import { readerLicenseUrl, readerSourceUrl } from "../../lib/thirdPartySoftware";
import { SystemSection } from "./SystemSection";

const tauriState = vi.hoisted(() => ({ isTauri: true }));
const probeFfmpegTools = vi.hoisted(() => vi.fn());
const setFfmpegToolsDir = vi.hoisted(() => vi.fn());
const getHomeDir = vi.hoisted(() => vi.fn());

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../../lib/tauri", () => ({
  probeFfmpegTools: (...args: unknown[]) => probeFfmpegTools(...args),
  setFfmpegToolsDir: (...args: unknown[]) => setFfmpegToolsDir(...args),
}));

vi.mock("../../lib/system-settings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/system-settings")>();
  return {
    ...actual,
    getHomeDir: () => getHomeDir(),
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  localStorage.clear();
  tauriState.isTauri = true;
  getHomeDir.mockResolvedValue("/home/demo");
  probeFfmpegTools.mockResolvedValue({
    ok: true,
    ffmpeg_path: "/usr/bin/ffmpeg",
    ffprobe_path: "/usr/bin/ffprobe",
    error: null,
  });
  setFfmpegToolsDir.mockResolvedValue({
    ok: true,
    ffmpeg_path: "/usr/bin/ffmpeg",
    ffprobe_path: "/usr/bin/ffprobe",
    error: null,
  });
});

describe("SystemSection", () => {
  it("shows this app's version in the browser and in the desktop app", async () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.getByText("Version").nextElementSibling).toHaveTextContent(APP_BUILD);
    cleanup();

    tauriState.isTauri = true;
    render(<SystemSection />);
    expect((await screen.findByText("Version")).nextElementSibling).toHaveTextContent(APP_BUILD);
  });

  it("names the Apple Messages reader and its GPL license in the desktop app only", async () => {
    tauriState.isTauri = true;
    render(<SystemSection />);
    expect(await screen.findByText("Third-party software")).toBeTruthy();
    expect(screen.getByText(/Apple Messages reader/)).toHaveTextContent("imessage-reader");
    expect(screen.getByText(/GNU General Public License/)).toBeTruthy();
    const source = screen.getByRole("link", { name: "Source" });
    expect(source.getAttribute("href")).toBe(readerSourceUrl(APP_BUILD));
    const license = screen.getByRole("link", { name: "License" });
    expect(license.getAttribute("href")).toBe(readerLicenseUrl(APP_BUILD));
    cleanup();

    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.queryByText("Third-party software")).toBeNull();
  });

  it("shows the desktop-only stub when not in Tauri", () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.getByText(/available in the desktop app/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  });

  it("has no Save button and labels the path fields", async () => {
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByLabelText("Staging directory")).toBeTruthy();
    });
    expect(screen.getByLabelText("ffmpeg directory")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Saving…" })).toBeNull();
  });

  it("persists the staging directory on change", async () => {
    const user = userEvent.setup();
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByDisplayValue("/home/demo/message-vault")).toBeTruthy();
    });

    const stagingInput = screen.getByDisplayValue("/home/demo/message-vault");
    await user.clear(stagingInput);
    await user.type(stagingInput, "/tmp/my-staging");

    expect(localStorage.getItem("mv-staging-dir")).toBe("/tmp/my-staging");
  });

  it("shows Found lines when both tools are present", async () => {
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByLabelText(/Found ffmpeg/i)).toBeTruthy();
    });
    expect(screen.getByLabelText(/Found ffprobe/i)).toBeTruthy();
    expect(screen.getByText("/usr/bin/ffmpeg")).toBeTruthy();
    expect(screen.getByText("/usr/bin/ffprobe")).toBeTruthy();
  });

  it("shows not-found when ffmpeg is missing", async () => {
    const missing = {
      ok: false,
      ffmpeg_path: null,
      ffprobe_path: "/usr/bin/ffprobe",
      error: "ffmpeg not found or failed -version",
    };
    probeFfmpegTools.mockResolvedValue(missing);
    setFfmpegToolsDir.mockResolvedValue(missing);
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByLabelText(/ffmpeg not found/i)).toBeTruthy();
    });
    expect(screen.getByLabelText(/Found ffprobe/i)).toBeTruthy();
  });

  it("does not persist an ffmpeg directory when the probe fails", async () => {
    const user = userEvent.setup();
    const missing = {
      ok: false,
      ffmpeg_path: null,
      ffprobe_path: null,
      error: "ffmpeg not found or failed -version",
    };
    probeFfmpegTools.mockResolvedValue(missing);
    setFfmpegToolsDir.mockResolvedValue(missing);
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByLabelText("ffmpeg directory")).toBeTruthy();
    });

    const ffmpegInput = screen.getByLabelText("ffmpeg directory");
    await user.type(ffmpegInput, "/opt/no-ffmpeg");
    await waitFor(() => {
      expect(probeFfmpegTools).toHaveBeenCalledWith("/opt/no-ffmpeg");
    });
    expect(localStorage.getItem("mv-ffmpeg-path")).toBeNull();
    expect(setFfmpegToolsDir).not.toHaveBeenCalledWith("/opt/no-ffmpeg");
  });

  it("keeps a previous ffmpeg directory when a later probe fails", async () => {
    const user = userEvent.setup();
    localStorage.setItem("mv-ffmpeg-path", "/usr/bin");
    render(<SystemSection />);
    await waitFor(() => {
      expect(screen.getByDisplayValue("/usr/bin")).toBeTruthy();
    });
    await waitFor(() => {
      expect(setFfmpegToolsDir).toHaveBeenCalledWith("/usr/bin");
    });

    const missing = {
      ok: false,
      ffmpeg_path: null,
      ffprobe_path: null,
      error: "ffmpeg not found or failed -version",
    };
    probeFfmpegTools.mockResolvedValue(missing);
    setFfmpegToolsDir.mockResolvedValue(missing);

    const ffmpegInput = screen.getByLabelText("ffmpeg directory");
    await user.type(ffmpegInput, "x");
    await waitFor(() => {
      expect(probeFfmpegTools).toHaveBeenCalledWith("/usr/binx");
    });
    expect(localStorage.getItem("mv-ffmpeg-path")).toBe("/usr/bin");
  });
});
