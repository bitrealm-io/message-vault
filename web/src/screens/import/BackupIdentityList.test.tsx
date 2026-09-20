/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import BackupIdentityList from "./BackupIdentityList";

afterEach(() => {
  cleanup();
});

const profile = { phones: ["+15550001111"], emails: [] };

describe("BackupIdentityList", () => {
  it("drops the box around each identity when shown as rows inside a stage", () => {
    const { rerender } = render(
      <BackupIdentityList identities={["+15550001111"]} profile={profile} onAdd={vi.fn()} />,
    );
    expect(screen.getByRole("listitem").className).toContain("border");
    rerender(
      <BackupIdentityList identities={["+15550001111"]} profile={profile} onAdd={vi.fn()} rows />,
    );
    expect(screen.getByRole("listitem").className).not.toContain("border");
    expect(screen.getByText("On your profile")).toBeInTheDocument();
  });

  it("marks matched addresses and offers to add unmatched ones", () => {
    render(
      <BackupIdentityList
        identities={["+15550001111", "owner@example.com"]}
        profile={profile}
        onAdd={vi.fn()}
      />,
    );
    expect(screen.getByText("+15550001111")).toBeInTheDocument();
    expect(screen.getByText("On your profile")).toBeInTheDocument();
    expect(screen.getByText("owner@example.com")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add to profile" })).toBeInTheDocument();
  });

  it("sends the value and its service to onAdd", async () => {
    const onAdd = vi.fn().mockResolvedValue(undefined);
    render(
      <BackupIdentityList identities={["owner@example.com"]} profile={profile} onAdd={onAdd} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Add to profile" }));
    expect(onAdd).toHaveBeenCalledWith("owner@example.com", "email");
  });

  it("states the fact when the backup records no identities", () => {
    render(<BackupIdentityList identities={[]} profile={profile} onAdd={vi.fn()} />);
    expect(
      screen.getByText("This backup doesn't record which account it came from."),
    ).toBeInTheDocument();
  });

  it("shows only the value while the profile hasn't loaded", () => {
    render(
      <BackupIdentityList identities={["owner@example.com"]} profile={null} onAdd={vi.fn()} />,
    );
    expect(screen.getByText("owner@example.com")).toBeInTheDocument();
    expect(screen.queryByText("On your profile")).not.toBeInTheDocument();
    expect(screen.queryByText("Not on your profile")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add to profile" })).not.toBeInTheDocument();
  });
});
