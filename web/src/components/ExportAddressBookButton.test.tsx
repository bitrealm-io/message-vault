/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveTextFile } from "../lib/saveTextFile";
import { exportAddressBook } from "../lib/serverApi";
import ExportAddressBookButton from "./ExportAddressBookButton";

vi.mock("../lib/serverApi", () => ({ exportAddressBook: vi.fn() }));
vi.mock("../lib/saveTextFile", () => ({ saveTextFile: vi.fn() }));

const exportMock = vi.mocked(exportAddressBook);
const saveMock = vi.mocked(saveTextFile);

const CSV =
  "contact_id,display_name,groups,service,identity_type,identity\n7,,,phone,phone,+15555550100\n";

describe("ExportAddressBookButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    exportMock.mockResolvedValue(CSV);
    saveMock.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
  });

  it("exports the list's search when no row is checked, and saves what the server answers", async () => {
    render(<ExportAddressBookButton query="group:Unknown" checkedIds={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    expect(exportMock).toHaveBeenCalledWith({ q: "group:Unknown" });
    expect(saveMock).toHaveBeenCalledWith("address-book.csv", CSV, "text/csv");
  });

  it("exports the checked rows alone when there are any", async () => {
    render(<ExportAddressBookButton query="ada" checkedIds={[4, 9]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    expect(exportMock).toHaveBeenCalledWith({ ids: [4, 9] });
  });

  it("says why when the export fails, and saves nothing", async () => {
    exportMock.mockRejectedValue(new Error("the search has a word Contacts does not know"));
    render(<ExportAddressBookButton query="nosuchword:1" checkedIds={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "the search has a word Contacts does not know",
    );
    expect(saveMock).not.toHaveBeenCalled();
  });
});
