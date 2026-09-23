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
  it("is a table inside a stage: sent and received per identity, and Add only where the answer is No", () => {
    render(
      <BackupIdentityList
        identities={["+15550001111", "owner@example.com"]}
        profile={profile}
        onAdd={vi.fn()}
        messageCounts={[
          // Two spellings of one number count under the same identity.
          { handle: "+15550001111", sent: 1200, received: 900 },
          { handle: "(555) 000-1111", sent: 34, received: 1 },
          { handle: "owner@example.com", sent: 0, received: 7 },
        ]}
      />,
    );
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual([
      "Identity",
      "Sent",
      "Received",
      "On your profile",
      "Action",
    ]);
    const [, phone, email] = screen.getAllByRole("row");
    expect(
      within(phone)
        .getAllByRole("cell")
        .map((cell) => cell.textContent),
    ).toEqual(["+15550001111", "1,234", "901", "Yes", ""]);
    expect(
      within(email)
        .getAllByRole("cell")
        .map((cell) => cell.textContent),
    ).toEqual(["owner@example.com", "0", "7", "No", "Add to profile"]);
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
