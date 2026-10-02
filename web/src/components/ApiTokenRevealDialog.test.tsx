/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import ApiTokenRevealDialog from "./ApiTokenRevealDialog";

function setClipboard(value: unknown) {
  Object.defineProperty(navigator, "clipboard", { value, configurable: true });
}

afterEach(() => {
  cleanup();
  delete (navigator as { clipboard?: unknown }).clipboard;
});

describe("ApiTokenRevealDialog", () => {
  it("says it could not copy when the browser has no clipboard, and shows the whole token", async () => {
    // Browsers expose navigator.clipboard only on HTTPS and localhost, so a
    // Message Crate opened at http://192.168.x.x has none.
    setClipboard(undefined);
    render(<ApiTokenRevealDialog open label="laptop" token="mc-pat-abc123" onClose={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This browser can't copy from this page. Select the token above and copy it.",
    );
    // A truncated token cannot be read off the screen, so it wraps instead.
    expect(screen.getByText("mc-pat-abc123")).toHaveClass("break-all");
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("copies the token and says so", async () => {
    const writeText = vi.fn(async () => {});
    setClipboard({ writeText });
    render(<ApiTokenRevealDialog open label="laptop" token="mc-pat-abc123" onClose={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith("mc-pat-abc123");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  // The secret can't be shown again, so only the dialog's own buttons close it.
  it.each([
    [
      "Escape",
      (user: ReturnType<typeof userEvent.setup>) => {
        screen.getByRole("dialog").focus();
        return user.keyboard("{Escape}");
      },
    ],
    ["a click outside it", (user: ReturnType<typeof userEvent.setup>) => user.click(document.body)],
  ])("stays open on %s", async (_name, dismiss) => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ApiTokenRevealDialog open label="laptop" token="mc-pat-abc123" onClose={onClose} />);

    await dismiss(user);

    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText("mc-pat-abc123")).toBeInTheDocument();
  });

  it("closes from its own two buttons", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ApiTokenRevealDialog open label="laptop" token="mc-pat-abc123" onClose={onClose} />);

    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(screen.getByRole("button", { name: "Done" }));

    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
