/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import PathPicker from "./PathPicker";

const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("PathPicker", () => {
  it("says the file dialog could not be opened when the dialog plugin rejects", async () => {
    // A Tauri command rejects with the plugin's own string, not an Error.
    dialog.open.mockRejectedValue("dialog unavailable");
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PathPicker value="" onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: "Browse" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The file dialog could not be opened: dialog unavailable",
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clears the message once a later Browse opens the dialog", async () => {
    dialog.open.mockRejectedValueOnce(new Error("dialog unavailable"));
    dialog.open.mockResolvedValueOnce("/backups/chat.db");
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PathPicker value="" onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: "Browse" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Browse" }));
    expect(onChange).toHaveBeenCalledWith("/backups/chat.db");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
