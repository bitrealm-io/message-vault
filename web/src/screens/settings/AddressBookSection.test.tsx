/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { loadAddressBook } from "../../lib/serverApi";
import { AddressBookSection } from "./AddressBookSection";

vi.mock("../../lib/serverApi", () => ({ loadAddressBook: vi.fn() }));

const invalidateAccount = vi.fn();
vi.mock("../../lib/routeQuery", () => ({
  useRouteCache: () => ({ invalidateAccount }),
}));

const post = vi.mocked(loadAddressBook);

const FILE = "contact_id,display_name,groups,service,identity_type,identity\n";

const NOTHING = {
  contacts_created: 0,
  contacts_updated: 0,
  contacts_deleted: 0,
  identities_added: 0,
  identities_moved: 0,
  identities_removed: 0,
  groups_created: 0,
  notes: [],
};

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

  it("offers Append and Edit, with Append chosen", () => {
    render(<AddressBookSection />);
    expect((screen.getByLabelText(/^Append/) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText(/^Edit/) as HTMLInputElement).checked).toBe(false);
  });

  it("sends the file's text as an Append unless Edit is chosen", async () => {
    post.mockResolvedValue(NOTHING);
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post).toHaveBeenLastCalledWith(FILE, "append");

    fireEvent.click(screen.getByLabelText(/^Edit/));
    chooseFile("address-book.csv", FILE);
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post).toHaveBeenLastCalledWith(FILE, "edit");
  });

  it("takes a .csv file only, and refuses a vCard without asking the server", async () => {
    render(<AddressBookSection />);
    expect(screen.getByLabelText("Address book file").getAttribute("accept")).toBe(".csv,text/csv");
    chooseFile("Contacts.vcf", "BEGIN:VCARD\nEND:VCARD\n");

    expect(await screen.findByText(/Choose a \.csv file/)).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });

  it("shows the seven counts of what the load changed", async () => {
    post.mockResolvedValue({
      contacts_created: 1,
      contacts_updated: 2,
      contacts_deleted: 3,
      identities_added: 4,
      identities_moved: 5,
      identities_removed: 6,
      groups_created: 7,
      notes: [],
    });
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    const counts = await screen.findByLabelText("What the load changed");
    const rows = within(counts)
      .getAllByRole("term")
      .map((term) => `${term.textContent}: ${term.nextElementSibling?.textContent}`);
    expect(rows).toEqual([
      "Contacts created: 1",
      "Contacts updated: 2",
      "Contacts deleted: 3",
      "Identities added: 4",
      "Identities moved: 5",
      "Identities removed: 6",
      "Contact Groups created: 7",
    ]);
  });

  it("lists each number the load read with its + back or made a new identity", async () => {
    const notes = [
      'row 2: 6591234567 has no +, so it was read as +6591234567, which "Ada" (contact 4) holds',
      "row 5: 447700900123 has no +, so it became the new identity 447700900123",
    ];
    post.mockResolvedValue({ ...NOTHING, notes });
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    const list = await screen.findByLabelText("Numbers written without +");
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(notes);
  });

  it("shows no list of numbers when every number kept its +", async () => {
    post.mockResolvedValue(NOTHING);
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    await screen.findByLabelText("What the load changed");
    expect(screen.queryByLabelText("Numbers written without +")).toBeNull();
  });

  it("marks the account's cache stale after a load", async () => {
    post.mockResolvedValue(NOTHING);
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    await waitFor(() => expect(invalidateAccount).toHaveBeenCalledTimes(1));
  });

  it("lists every row a refused load names, each on its own line", async () => {
    post.mockRejectedValue(
      new ApiError(422, "row 3: service; row 7: identity", {
        type: "https://messagecrate.app/docs/developer/reference/errors/validation-failed",
        title: "Validation failed",
        status: 422,
        errors: [
          'row 3: service "imessage" is not one Message Crate stores; use phone or whatsapp',
          "row 7: identity is blank",
        ],
      }),
    );
    render(<AddressBookSection />);
    chooseFile("address-book.csv", FILE);

    const alert = await screen.findByRole("alert");
    expect(
      within(alert)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual([
      'row 3: service "imessage" is not one Message Crate stores; use phone or whatsapp',
      "row 7: identity is blank",
    ]);
    expect(alert.textContent).toContain("Nothing was loaded.");
    expect(invalidateAccount).not.toHaveBeenCalled();
  });

  it("shows the reason when the load fails some other way", async () => {
    post.mockRejectedValue(new Error("address book is empty"));
    render(<AddressBookSection />);
    chooseFile("address-book.csv", "  ");

    expect(await screen.findByText("address book is empty")).toBeInTheDocument();
  });

  it("refuses a file past the size the server accepts, without asking the server", async () => {
    render(<AddressBookSection />);
    const input = screen.getByLabelText("Address book file") as HTMLInputElement;
    const file = new File(["x"], "huge.csv");
    Object.defineProperty(file, "size", { value: 9 * 1024 * 1024 });
    fireEvent.change(input, { target: { files: [file] } });

    expect(await screen.findByText("That file is larger than 8 MB.")).toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
  });
});
