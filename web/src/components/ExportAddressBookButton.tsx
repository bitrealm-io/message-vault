import { useState } from "react";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { saveTextFile } from "../lib/saveTextFile";
import { exportAddressBook } from "../lib/serverApi";
import Button from "./Button";

/** The name the address book is saved under, the one the server gives it. */
const FILE_NAME = "address-book.csv";

/**
 * Export the address book for the contacts the list is showing.
 *
 * Checked rows are the file when there are any. Otherwise the list's search
 * is, and an empty search is every contact. The file is the one Settings
 * loads back, so a person can name fifty Unknowns in a spreadsheet at once.
 */
export default function ExportAddressBookButton({
  query,
  checkedIds,
}: {
  /** The list's search as the server takes it, group filter included. */
  query: string;
  /** Ids of the checked rows. */
  checkedIds: number[];
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const run = async () => {
    setBusy(true);
    setError("");
    try {
      // The checked rows are rows the person can see, so they are the whole
      // selection: sending the search with them could only take rows away.
      const body = checkedIds.length > 0 ? { ids: checkedIds } : { q: query };
      const csv = await exportAddressBook(body);
      await saveTextFile(FILE_NAME, csv, "text/csv");
    } catch (err) {
      setError(apiErrorMessage(err, "Could not export the address book."));
    } finally {
      setBusy(false);
    }
  };

  const what =
    checkedIds.length > 0
      ? "the checked contacts"
      : query.trim()
        ? "the contacts this list shows"
        : "every contact";
  return (
    <span className="flex items-center gap-2">
      {error ? (
        <span role="alert" className="text-[0.75rem] text-danger">
          {error}
        </span>
      ) : null}
      <Button
        type="button"
        variant="ghost"
        size="chip"
        disabled={busy}
        title={`Export ${what} as an address book you can edit in a spreadsheet and load back under Settings`}
        onClick={() => void run()}
      >
        {busy ? "Exporting…" : "Export"}
      </Button>
    </span>
  );
}
