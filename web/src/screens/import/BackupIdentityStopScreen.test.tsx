/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import BackupIdentityStopScreen from "./BackupIdentityStopScreen";

const noMatch = { phones: ["+15555550180"], emails: [] };

describe("BackupIdentityStopScreen", () => {
  afterEach(cleanup);

  it("states the mismatch and offers Continue and Cancel — no checkbox", () => {
    render(
      <BackupIdentityStopScreen
        identities={["+15555550110"]}
        profile={noMatch}
        onAdd={vi.fn()}
        onContinue={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(
      screen.getByText("None of the addresses this backup sent from are on your profile."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue import" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("restates the fact once an address matches (after an add)", () => {
    render(
      <BackupIdentityStopScreen
        identities={["+15555550110"]}
        profile={{ phones: ["+15555550110"], emails: [] }}
        onAdd={vi.fn()}
        onContinue={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(
      screen.getByText("An address this backup sent from is on your profile."),
    ).toBeInTheDocument();
  });

  it("wires the two buttons", async () => {
    const onContinue = vi.fn();
    const onCancel = vi.fn();
    render(
      <BackupIdentityStopScreen
        identities={["+15555550110"]}
        profile={noMatch}
        onAdd={vi.fn()}
        onContinue={onContinue}
        onCancel={onCancel}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Continue import" }));
    expect(onContinue).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
  });
});
