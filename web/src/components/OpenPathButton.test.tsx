/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import OpenPathButton from "./OpenPathButton";

const openPathInExplorer = vi.fn();

vi.mock("../lib/openPath", () => ({
  openPathInExplorer: (...args: unknown[]) => openPathInExplorer(...args),
}));

describe("OpenPathButton", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    openPathInExplorer.mockReset();
  });

  it("opens the path when clicked", async () => {
    const user = userEvent.setup();
    openPathInExplorer.mockResolvedValue(undefined);
    render(<OpenPathButton path="/home/sam/message-crate/staging">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(openPathInExplorer).toHaveBeenCalledWith("/home/sam/message-crate/staging");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an alert when opening fails", async () => {
    const user = userEvent.setup();
    openPathInExplorer.mockRejectedValue(new Error("Path is outside the import staging folder"));
    render(<OpenPathButton path="/tmp/nope">Open</OpenPathButton>);
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Path is outside the import staging folder",
    );
  });
});
