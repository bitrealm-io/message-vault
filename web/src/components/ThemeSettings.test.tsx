/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const seeds = {
  lightHeader: "#112233",
  lightAccent: "#445566",
  darkHeader: "#778899",
  darkAccent: "#aabbcc",
};
const shareString = "#112233,#445566,#778899,#aabbcc";

vi.mock("../lib/ThemeProvider", () => ({
  useTheme: () => ({
    mode: "light",
    setMode: vi.fn(),
    seeds,
    patchSeed: vi.fn(),
    shareString,
    setShareString: vi.fn(() => true),
    applyPreset: vi.fn(),
    resolvedMode: "light",
    presets: [],
  }),
}));

import ThemeSettings from "./ThemeSettings";

function setClipboard(value: unknown) {
  Object.defineProperty(navigator, "clipboard", { value, configurable: true });
}

afterEach(() => {
  cleanup();
  delete (navigator as { clipboard?: unknown }).clipboard;
});

describe("ThemeSettings share theme", () => {
  it("says it could not copy when the browser has no clipboard, and keeps the codes in the field", async () => {
    // Browsers expose navigator.clipboard only on HTTPS and localhost, so a
    // Message Crate opened at http://192.168.x.x has none.
    setClipboard(undefined);
    render(<ThemeSettings />);

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This browser can't copy from this page. Select the color codes above and copy them.",
    );
    expect(screen.getByDisplayValue(shareString)).toBeInTheDocument();
  });

  it("copies the color codes and says so", async () => {
    const writeText = vi.fn(async () => {});
    setClipboard({ writeText });
    render(<ThemeSettings />);

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith(shareString);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
