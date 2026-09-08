import { useRef, useState } from "react";
import Button from "../../components/Button";
import { useContactGroupActions } from "../../lib/contactGroups";
import { addressBookContentType, loadAddressBook } from "../../lib/vaultApi";
import { sectionTitleClass } from "./profileStyles";

/** Largest file the server accepts, mirrored here so the refusal is immediate. */
const MAX_BYTES = 8 * 1024 * 1024;

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/**
 * Load a VCF or vCard CSV address book into the vault.
 *
 * This is its own act, not part of an import run: contacts are vault state, and
 * a person may load them before or after bringing messages in. Loading again
 * refreshes the file's own entries and leaves Contact Groups, names typed by
 * hand, and contacts discovered from messages alone.
 */
export function AddressBookSection() {
  const groupActions = useContactGroupActions();
  const fileRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  const load = async (file: File) => {
    setBusy(true);
    setMessage("");
    setFailed(false);
    try {
      if (file.size > MAX_BYTES) {
        setFailed(true);
        setMessage("That file is larger than 8 MB.");
        return;
      }
      const contentType = addressBookContentType(file.name);
      if (!contentType) {
        setFailed(true);
        setMessage("Choose a .vcf or .csv file.");
        return;
      }
      const content = await file.text();
      const res = await loadAddressBook(content, contentType);
      const review =
        res.phones_needing_review > 0
          ? `, ${plural(res.phones_needing_review, "number needs", "numbers need")} a look`
          : "";
      setMessage(
        `Loaded ${plural(res.contacts, "contact", "contacts")} and ${plural(
          res.phones,
          "phone number",
          "phone numbers",
        )}${review}.`,
      );
      void groupActions.invalidate();
    } catch (err) {
      setFailed(true);
      setMessage(err instanceof Error ? err.message : "Could not load that address book.");
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  return (
    <section>
      <h2 className={sectionTitleClass}>Address book</h2>
      <p className="mb-3 text-[0.813rem] text-muted">
        Load a VCF or vCard CSV to put names to the phone numbers in your messages. Loading again
        refreshes these entries and leaves your contact groups, the names you typed, and the
        contacts found in your messages as they are.
      </p>
      <input
        ref={fileRef}
        type="file"
        accept=".vcf,.vcard,.csv"
        className="hidden"
        aria-label="Address book file"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) void load(file);
        }}
      />
      <Button type="button" disabled={busy} onClick={() => fileRef.current?.click()}>
        {busy ? "Loading…" : "Choose a file"}
      </Button>
      {message ? (
        <p className={`mt-3 text-[0.813rem] ${failed ? "text-danger" : "text-muted"}`}>{message}</p>
      ) : null}
    </section>
  );
}
