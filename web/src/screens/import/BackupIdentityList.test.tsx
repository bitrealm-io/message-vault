/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import BackupIdentityList from "./BackupIdentityList";

afterEach(() => {
  cleanup();
});

const profile = { phones: ["+15550001111"], emails: [] };

describe("BackupIdentityList", () => {
  it("is a table inside a stage: messages per identity, and Add only where the answer is No", () => {
    render(
      <BackupIdentityList
        identities={["+15550001111", "owner@example.com"]}
        profile={profile}
        onAdd={vi.fn()}
        messageCounts={[
          // Two spellings of one number count under the same identity.
          { handle: "+15550001111", messages: 1200 },
          { handle: "(555) 000-1111", messages: 34 },
          { handle: "owner@example.com", messages: 7 },
        ]}
      />,
    );
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual([
      "Identity",
      "Messages",
      "On your profile",
      "Action",
    ]);
    const [, phone, email] = screen.getAllByRole("row");
    expect(
      within(phone)
        .getAllByRole("cell")
        .map((cell) => cell.textContent),
    ).toEqual(["+15550001111", "1,234", "Yes", ""]);
    expect(within(email).getByText("7")).toBeInTheDocument();
    expect(within(email).getByText("No")).toBeInTheDocument();
    expect(within(email).getByRole("button", { name: "Add to profile" })).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
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
