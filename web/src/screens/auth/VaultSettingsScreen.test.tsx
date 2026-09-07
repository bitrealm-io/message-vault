/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import VaultSettingsScreen from "./VaultSettingsScreen";
import type { VaultConnection } from "./VaultStatus";

function renderScreen(overrides: Partial<Parameters<typeof VaultSettingsScreen>[0]> = {}) {
  const props = {
    draft: "http://127.0.0.1:8080",
    status: "connected" as VaultConnection,
    canSubmit: true,
    onDraftChange: vi.fn(),
    onTest: vi.fn(),
    onCancel: vi.fn(),
    onSubmit: vi.fn(),
    ...overrides,
  };
  render(<VaultSettingsScreen {...props} />);
  return props;
}

describe("VaultSettingsScreen", () => {
  afterEach(cleanup);

  it("names itself and the field without repeating the word vault", () => {
    renderScreen();

    expect(screen.getByRole("heading", { name: "Message Vault Settings" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("http://127.0.0.1:8080");
    expect(screen.getByText("Connection Status")).toBeInTheDocument();
  });

  it("reports the status it is handed", () => {
    renderScreen({ status: "disconnected" });
    expect(screen.getByText("Disconnected")).toBeInTheDocument();
  });

  it("tests the typed address on the button and on Enter", async () => {
    const user = userEvent.setup();
    const props = renderScreen();

    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(props.onTest).toHaveBeenCalledTimes(1);

    await user.type(screen.getByRole("textbox", { name: "Address" }), "{Enter}");
    expect(props.onTest).toHaveBeenCalledTimes(2);
  });

  it("keeps applying the address available without testing first", async () => {
    const user = userEvent.setup();
    const props = renderScreen();

    const apply = screen.getByRole("button", { name: "Change vault address" });
    expect(apply).toBeEnabled();
    await user.click(apply);
    expect(props.onSubmit).toHaveBeenCalledOnce();
  });

  it("does not offer a change the caller says is not a change", async () => {
    const user = userEvent.setup();
    const props = renderScreen({ canSubmit: false });

    const apply = screen.getByRole("button", { name: "Change vault address" });
    expect(apply).toBeDisabled();
    await user.click(apply);
    expect(props.onSubmit).not.toHaveBeenCalled();

    // Re-probing the address in the field is still a real question to ask.
    expect(screen.getByRole("button", { name: "Test" })).toBeEnabled();
  });

  it("offers a way out that changes nothing", async () => {
    const user = userEvent.setup();
    const props = renderScreen();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(props.onCancel).toHaveBeenCalledOnce();
    expect(props.onSubmit).not.toHaveBeenCalled();
  });
});
