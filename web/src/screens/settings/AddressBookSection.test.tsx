/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadAddressBook } from "../../lib/vaultApi";
import { AddressBookSection } from "./AddressBookSection";

vi.mock("../../lib/vaultApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/vaultApi")>();
  return { addressBookContentType: actual.addressBookContentType, loadAddressBook: vi.fn() };
});

vi.mock("../../lib/contactGroups", () => ({
  useContactGroupActions: () => ({ invalidate: vi.fn() }),
}));

const post = vi.mocked(loadAddressBook);

function chooseFile(name: string, body: string) {
  const input = screen.getByLabelText("Address book file") as HTMLInputElement;
  const file = new File([body], name, { type: "text/plain" });
  fireEvent.change(input, { target: { files: [file] } });
  return file;
}

describe("AddressBookSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("sends the file's text under the media type its name earns", async () => {
    post.mockResolvedValue({ contacts: 2, phones: 3, phones_needing_review: 0 });
    render(<AddressBookSection />);
    chooseFile("Contacts.vcf", "BEGIN:VCARD\nEND:VCARD\n");

    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post).toHaveBeenCalledWith("BEGIN:VCARD\nEND:VCARD\n", "text/vcard");
  });

  it("refuses a file that is neither vCard nor CSV, without asking the server", async () => {
    render(<AddressBookSection />);
    chooseFile("contacts.json", "[]");

    expect(await screen.findByText("Choose a .vcf or .csv file.")).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });

  it("reports what the load changed", async () => {
    post.mockResolvedValue({ contacts: 2, phones: 3, phones_needing_review: 0 });
    render(<AddressBookSection />);
    chooseFile("Contacts.vcf", "BEGIN:VCARD\nEND:VCARD\n");

    expect(await screen.findByText("Loaded 2 contacts and 3 phone numbers.")).toBeInTheDocument();
  });

  it("says when numbers need a look", async () => {
    post.mockResolvedValue({ contacts: 1, phones: 1, phones_needing_review: 1 });
    render(<AddressBookSection />);
    chooseFile("Contacts.vcf", "BEGIN:VCARD\nEND:VCARD\n");

    expect(
      await screen.findByText("Loaded 1 contact and 1 phone number, 1 number needs a look."),
    ).toBeInTheDocument();
  });

  it("shows the reason when the load fails", async () => {
    post.mockRejectedValue(new Error("address book is empty"));
    render(<AddressBookSection />);
    chooseFile("Contacts.vcf", "  ");

    expect(await screen.findByText("address book is empty")).toBeInTheDocument();
  });

  it("refuses a file past the size the server accepts, without asking the server", async () => {
    render(<AddressBookSection />);
    const input = screen.getByLabelText("Address book file") as HTMLInputElement;
    const file = new File(["x"], "huge.vcf");
    Object.defineProperty(file, "size", { value: 9 * 1024 * 1024 });
    fireEvent.change(input, { target: { files: [file] } });

    expect(await screen.findByText("That file is larger than 8 MB.")).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });
});
