import { useRef, useState } from "react";
import Button from "../../components/Button";
import { ApiError } from "../../lib/api";
import { keys } from "../../lib/queryKeys";
import { useRouteCache } from "../../lib/routeQuery";
import { type AddressBookLoadMode, loadAddressBook } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { sectionTitleClass } from "./profileStyles";

/** Largest file the server accepts, mirrored here so the refusal is immediate. */
const MAX_BYTES = 8 * 1024 * 1024;

type LoadCounts = components["schemas"]["LoadCounts"];

const MODES: ReadonlyArray<{ value: AddressBookLoadMode; label: string; detail: string }> = [
  {
    value: "append",
    label: "Append",
    detail:
      "Creates the contacts in the file, renames the ones it holds, and adds the identities and Contact Groups it lists. Nothing is removed.",
  },
  {
    value: "edit",
    label: "Edit",
    detail:
      "Does the same, then makes each contact in the file hold exactly the identities and Contact Groups its rows list. A row taken out of the file takes that identity off the contact.",
  },
];

/** The seven counts a load answers, in the order the result lists them. */
const COUNTS: ReadonlyArray<{ field: keyof LoadCounts; label: string }> = [
  { field: "contacts_created", label: "Contacts created" },
  { field: "contacts_updated", label: "Contacts updated" },
  { field: "contacts_deleted", label: "Contacts deleted" },
  { field: "identities_added", label: "Identities added" },
  { field: "identities_moved", label: "Identities moved" },
  { field: "identities_removed", label: "Identities removed" },
  { field: "groups_created", label: "Contact Groups created" },
];

/** Every sentence of a refused load: one for each bad row. */
function refusalLines(err: unknown): string[] {
  if (err instanceof ApiError) {
    const errors = err.errors.map((e) => e.trim()).filter(Boolean);
    if (errors.length > 0) return errors;
  }
  return [err instanceof Error && err.message ? err.message : "Could not load that address book."];
}

/**
 * Load an address book: the CSV that Export on the Contacts screen writes,
 * edited in a spreadsheet and put back.
 *
 * Contacts arrive with message imports, so this is not where they come from.
 * It is how a person names, regroups, and corrects many of them at once. A
 * contact the file does not mention is left alone, and a file with a bad row
 * is refused whole, with each row and its reason listed.
 */
export function AddressBookSection() {
  const cache = useRouteCache();
  const fileRef = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<AddressBookLoadMode>("append");
  const [busy, setBusy] = useState(false);
  const [counts, setCounts] = useState<LoadCounts | null>(null);
  const [errors, setErrors] = useState<string[]>([]);

  const load = async (file: File) => {
    setBusy(true);
    setCounts(null);
    setErrors([]);
    try {
      if (!file.name.trim().toLowerCase().endsWith(".csv")) {
        setErrors(["Choose a .csv file: the address book Export on the Contacts screen writes."]);
        return;
      }
      if (file.size > MAX_BYTES) {
        setErrors(["That file is larger than 8 MB."]);
        return;
      }
      const content = await file.text();
      setCounts(await loadAddressBook(content, mode));
      // A load renames contacts and moves identities, so every list that
      // shows a contact's name or its groups is stale.
      void cache.invalidate(keys.contacts.all, keys.contactGroups.all, keys.conversations.all);
    } catch (err) {
      setErrors(refusalLines(err));
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  return (
    <section>
      <h3 className={sectionTitleClass}>Address book</h3>
      <p className="mb-3 text-[0.813rem] text-muted">
        Export on the Contacts screen writes your contacts as a CSV file. Edit it in a spreadsheet,
        then load it here. Contacts the file does not mention stay as they are.
      </p>
      <fieldset className="mb-3 flex flex-col gap-2 border-0 p-0">
        <legend className="mb-1 p-0 text-[0.813rem] font-medium">How to load it</legend>
        {MODES.map((option) => (
          <label key={option.value} className="flex cursor-pointer items-start gap-2">
            <input
              type="radio"
              name="address-book-mode"
              className="mt-1"
              value={option.value}
              checked={mode === option.value}
              disabled={busy}
              onChange={() => setMode(option.value)}
            />
            <span className="text-[0.813rem]">
              <span className="font-medium">{option.label}</span>
              <span className="block text-muted">{option.detail}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <input
        ref={fileRef}
        type="file"
        accept=".csv,text/csv"
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
      {counts ? (
        <dl
          aria-label="What the load changed"
          className="mt-3 grid max-w-xs grid-cols-[1fr_auto] gap-x-4 gap-y-1 text-[0.813rem]"
        >
          {COUNTS.map(({ field, label }) => (
            <div key={field} className="contents">
              <dt className="text-muted">{label}</dt>
              <dd className="m-0 text-right tabular-nums">{counts[field].toLocaleString()}</dd>
            </div>
          ))}
        </dl>
      ) : null}
      {errors.length > 0 ? (
        <div role="alert" className="mt-3 text-[0.813rem] text-danger">
          {errors.length > 1 ? (
            <p className="m-0 mb-1">
              Nothing was loaded. Fix these {errors.length.toLocaleString()} rows and load the file
              again:
            </p>
          ) : null}
          <ul className="m-0 list-none p-0">
            {errors.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
