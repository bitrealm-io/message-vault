/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setBaseUrl } from "../../lib/api";
import { APP_BUILD } from "../../lib/build";
import { getOpenToNetwork } from "../../lib/localServer";
import { readerLicenseUrl, readerSourceUrl } from "../../lib/thirdPartySoftware";
import { SystemSection } from "./SystemSection";

const tauriState = vi.hoisted(() => ({ isTauri: true }));
const probeFfmpegTools = vi.hoisted(() => vi.fn());
const setFfmpegToolsDir = vi.hoisted(() => vi.fn());
/** The Staging Directory as the desktop process keeps it. */
const desktopStaging = vi.hoisted(() => ({ root: "", defaultRoot: "/home/demo/message-crate" }));
const setStagingRoot = vi.hoisted(() => vi.fn());
const openDataFolder = vi.hoisted(() => vi.fn());

const startLocalServer = vi.hoisted(() => vi.fn());
const setLocalServerOpenToNetwork = vi.hoisted(() => vi.fn());

vi.mock("../../lib/localServer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/localServer")>()),
  openDataFolder: () => openDataFolder(),
  startLocalServer: () => startLocalServer(),
  setLocalServerOpenToNetwork: (on: boolean) => setLocalServerOpenToNetwork(on),
}));

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../../lib/tauri", () => ({
  probeFfmpegTools: (...args: unknown[]) => probeFfmpegTools(...args),
  setFfmpegToolsDir: (...args: unknown[]) => setFfmpegToolsDir(...args),
  invokeStagingRoot: async () => ({
    root: desktopStaging.root || desktopStaging.defaultRoot,
    defaultRoot: desktopStaging.defaultRoot,
  }),
  invokeSetStagingRoot: (root: string) => setStagingRoot(root),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  setBaseUrl("");
});

beforeEach(() => {
  localStorage.clear();
  tauriState.isTauri = true;
  desktopStaging.root = "";
  setStagingRoot.mockReset();
  setStagingRoot.mockImplementation(async (root: string) => {
    // The desktop process refuses a relative folder, as resolve_staging_root does.
    if (root !== "" && !root.startsWith("/")) throw "The staging directory must be a full path.";
    desktopStaging.root = root === desktopStaging.defaultRoot ? "" : root;
    return {
      root: desktopStaging.root || desktopStaging.defaultRoot,
      defaultRoot: desktopStaging.defaultRoot,
    };
  });
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

  it("opens the data folder of the app's own Message Crate", async () => {
    openDataFolder.mockResolvedValue(undefined);
    render(<SystemSection />);
    await userEvent.click(await screen.findByRole("button", { name: "Open data folder" }));
    expect(openDataFolder).toHaveBeenCalledTimes(1);
  });

  it("says why the data folder could not be opened", async () => {
    openDataFolder.mockRejectedValue(new Error("Could not open /data"));
    render(<SystemSection />);
    await userEvent.click(await screen.findByRole("button", { name: "Open data folder" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not open /data");
  });

  it("keeps the app's own Message Crate closed to the network until asked", async () => {
    setBaseUrl("http://127.0.0.1:8080");
    startLocalServer.mockReset();
    setLocalServerOpenToNetwork.mockReset();
    setLocalServerOpenToNetwork.mockResolvedValue({ status: "starting", first_time: false });
    render(<SystemSection />);
    const box = await screen.findByRole("checkbox", {
      name: /Let other devices on this network connect/,
    });
    expect(box).not.toBeChecked();
    // The warning is there before the choice, not after it.
    expect(screen.getByText(/plain HTTP/)).toBeInTheDocument();

    await userEvent.click(box);

    expect(getOpenToNetwork()).toBe(true);
    expect(setLocalServerOpenToNetwork).toHaveBeenLastCalledWith(true);

    await userEvent.click(box);
    expect(getOpenToNetwork()).toBe(false);
    expect(setLocalServerOpenToNetwork).toHaveBeenLastCalledWith(false);
    // The setting restarts a server the app runs; it never starts one.
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("changes no server while the app uses another Message Crate", async () => {
    setBaseUrl("https://crate.example");
    startLocalServer.mockReset();
    setLocalServerOpenToNetwork.mockReset();
    render(<SystemSection />);

    await userEvent.click(
      await screen.findByRole("checkbox", { name: /Let other devices on this network connect/ }),
    );

    expect(getOpenToNetwork()).toBe(true);
    expect(setLocalServerOpenToNetwork).not.toHaveBeenCalled();
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("says the setting does not change a Message Crate the app did not start", async () => {
    setBaseUrl("http://127.0.0.1:8080");
    setLocalServerOpenToNetwork.mockReset();
    setLocalServerOpenToNetwork.mockResolvedValue({ status: "ready", started_by_app: false });
    render(<SystemSection />);
    await userEvent.click(
      await screen.findByRole("checkbox", { name: /Let other devices on this network connect/ }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("This app did not start");
  });

  it("offers no data folder in the browser", () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.queryByRole("button", { name: "Open data folder" })).toBeNull();
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

  it("stores the staging directory in the desktop process on change", async () => {
    const user = userEvent.setup();
    render(<SystemSection />);
    const stagingInput = await screen.findByDisplayValue("/home/demo/message-crate");

    await user.clear(stagingInput);
    await user.type(stagingInput, "/tmp/my-staging");

    await waitFor(() => expect(desktopStaging.root).toBe("/tmp/my-staging"));
    expect(setStagingRoot).toHaveBeenLastCalledWith("/tmp/my-staging");
  });

  it("says why the desktop process refused a staging directory", async () => {
    const user = userEvent.setup();
    render(<SystemSection />);
    const stagingInput = await screen.findByDisplayValue("/home/demo/message-crate");
    setStagingRoot.mockRejectedValue("Could not save staging.json: disk full");

    await user.type(stagingInput, "/x");

    expect(await screen.findByText(/disk full/)).toBeInTheDocument();
  });

  it("says why a relative staging directory is not saved, and shows the one in use on blur", async () => {
    const user = userEvent.setup();
    desktopStaging.root = "/srv/staging";
    render(<SystemSection />);
    const stagingInput = await screen.findByDisplayValue("/srv/staging");

    // Typed over the whole path: an empty field would clear the setting on its own.
    await user.type(stagingInput, "staging", {
      initialSelectionStart: 0,
      initialSelectionEnd: "/srv/staging".length,
    });

    expect(stagingInput).toHaveValue("staging");
    expect(
      await screen.findByText("Not saved. The staging directory must be a full path."),
    ).toBeInTheDocument();
    expect(desktopStaging.root).toBe("/srv/staging");

    await user.tab();

    expect(stagingInput).toHaveValue("/srv/staging");
    expect(screen.queryByText(/must be a full path/)).toBeNull();
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
    expect(localStorage.getItem("mc-ffmpeg-path")).toBeNull();
    expect(setFfmpegToolsDir).not.toHaveBeenCalledWith("/opt/no-ffmpeg");
  });

  it("keeps a previous ffmpeg directory when a later probe fails", async () => {
    const user = userEvent.setup();
    localStorage.setItem("mc-ffmpeg-path", "/usr/bin");
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
    expect(localStorage.getItem("mc-ffmpeg-path")).toBe("/usr/bin");
  });
});
