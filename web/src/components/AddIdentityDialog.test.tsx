/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import AddIdentityDialog from "./AddIdentityDialog";

afterEach(cleanup);

describe("AddIdentityDialog", () => {
  it("offers Text message, Email and WhatsApp, and hands back the service picked", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(<AddIdentityDialog open onClose={() => {}} onConfirm={onConfirm} />);

    await user.click(screen.getByRole("button", { name: /Service/ }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Text message",
      "Email",
      "WhatsApp",
    ]);
    await user.click(screen.getByRole("option", { name: "Email" }));
    expect(screen.getByRole("textbox", { name: "Identity" })).toHaveAttribute(
      "placeholder",
      "you@example.com",
    );
    await user.type(screen.getByRole("textbox", { name: "Identity" }), "Bob@Example.com");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(onConfirm).toHaveBeenCalledWith({ handle: "Bob@Example.com", service: "email" });
  });

  it("refuses a value that is not a number or an address, before asking anyone", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(<AddIdentityDialog open onClose={() => {}} onConfirm={onConfirm} />);

    await user.type(screen.getByRole("textbox", { name: "Identity" }), "12");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Enter a phone number like +1 555-123-4567.",
    );
    expect(onConfirm).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await user.clear(screen.getByRole("textbox", { name: "Identity" }));
    await user.type(screen.getByRole("textbox", { name: "Identity" }), "not-an-address");
    await user.keyboard("{Enter}");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Enter an email address like you@example.com.",
    );
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("refuses an identity already in the list, on the same service only", async () => {
    const user = userEvent.setup({ delay: null });
    const onConfirm = vi.fn();
    render(
      <AddIdentityDialog
        open
        existing={[{ handle: "+15555550100", service: "phone" }]}
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    await user.type(screen.getByRole("textbox", { name: "Identity" }), "+1 (555) 555-0100");
    expect(screen.getByRole("alert")).toHaveTextContent("This identity is already in the list.");
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "WhatsApp" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(onConfirm).toHaveBeenCalledWith({ handle: "+1 (555) 555-0100", service: "whatsapp" });
  });

  it("shows why the last attempt failed and stays open while busy", () => {
    render(
      <AddIdentityDialog
        open
        busy
        error="The vault did not add that identity."
        onClose={() => {}}
        onConfirm={() => {}}
      />,
    );
    expect(screen.getByText("The vault did not add that identity.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });
});
