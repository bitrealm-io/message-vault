import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const isTauri = vi.fn();
const resolveStagingParent = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

vi.mock("./tauri-check", () => ({
  isTauri: () => isTauri(),
}));

vi.mock("./system-settings", () => ({
  resolveStagingParent: (...args: unknown[]) => resolveStagingParent(...args),
}));

describe("openPathInExplorer", () => {
  beforeEach(() => {
    invoke.mockReset();
    isTauri.mockReset();
    resolveStagingParent.mockReset();
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(undefined);
    resolveStagingParent.mockResolvedValue("/home/sam/message-crate");
  });

  it("invokes open_path with the staging parent in the desktop app", async () => {
    const { openPathInExplorer } = await import("./openPath");
    await openPathInExplorer("/home/sam/message-crate/staging");
    expect(invoke).toHaveBeenCalledWith("open_path", {
      path: "/home/sam/message-crate/staging",
      stagingRoot: "/home/sam/message-crate",
    });
  });

  it("no-ops on blank paths", async () => {
    const { openPathInExplorer } = await import("./openPath");
    await openPathInExplorer("   ");
    expect(invoke).not.toHaveBeenCalled();
    expect(resolveStagingParent).not.toHaveBeenCalled();
  });

  it("rejects when not running in Tauri", async () => {
    isTauri.mockReturnValue(false);
    const { openPathInExplorer } = await import("./openPath");
    await expect(openPathInExplorer("/home/sam/message-crate/staging")).rejects.toThrow(
      "desktop app",
    );
  });

  it("rejects when the staging parent cannot be resolved", async () => {
    resolveStagingParent.mockResolvedValue("");
    const { openPathInExplorer } = await import("./openPath");
    await expect(openPathInExplorer("/home/sam/message-crate/staging")).rejects.toThrow(
      /staging directory/i,
    );
  });
});
