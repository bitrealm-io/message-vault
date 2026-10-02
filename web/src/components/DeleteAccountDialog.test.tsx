/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import DeleteAccountDialog from "./DeleteAccountDialog";

afterEach(cleanup);

const deleteButton = () => screen.getByRole("button", { name: "Permanently delete my account" });

describe("DeleteAccountDialog", () => {
  it("asks an account with no password for its username only", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(
      <DeleteAccountDialog
        open
        username="carol"
        hasPassword={false}
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    expect(screen.queryByLabelText("Current password")).toBeNull();
    expect(deleteButton()).toBeDisabled();

    await user.type(screen.getByRole("textbox", { name: /Type your username carol/ }), "carol");
    expect(deleteButton()).toBeEnabled();
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledWith(undefined);
  });

  it("requires the current password from an account that has one", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(
      <DeleteAccountDialog
        open
        username="carol"
        hasPassword
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    await user.type(screen.getByRole("textbox", { name: /Type your username carol/ }), "carol");
    expect(deleteButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Current password"), "hunter2");
    expect(deleteButton()).toBeEnabled();
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledWith("hunter2");
  });

  /**
   * The typed password must not outlive the dialog. While closed nothing shows
   * it, so this watches the reopen: any moment the field holds the old
   * password, even before an effect clears it, is recorded in the DOM.
   */
  it("forgets the typed password when it closes", async () => {
    const user = userEvent.setup({ delay: null });
    const dialog = (open: boolean) => (
      <DeleteAccountDialog
        open={open}
        username="carol"
        hasPassword
        onClose={() => {}}
        onConfirm={() => {}}
      />
    );
    const { rerender } = render(dialog(true));
    await user.type(screen.getByLabelText("Current password"), "hunter2");

    rerender(dialog(false));
    const seen: string[] = [];
    const observer = new MutationObserver((records) => {
      for (const r of records) {
        if (r.oldValue) seen.push(r.oldValue);
        for (const node of r.addedNodes) {
          if (node instanceof Element) {
            for (const input of node.querySelectorAll("input")) seen.push(input.value);
          }
        }
      }
    });
    observer.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["value"],
      attributeOldValue: true,
    });
    rerender(dialog(true));
    await Promise.resolve();
    observer.disconnect();

    expect(screen.getByLabelText("Current password")).toHaveValue("");
    expect(seen).not.toContain("hunter2");
  });
});
