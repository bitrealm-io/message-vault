import { useEffect, useState } from "react";
import { getImportContacts } from "../../../lib/vaultApi";
import type { components } from "../../../lib/vaultApi.types";

/** What the run did to one contact, as the vault recorded it. */
type ContactReason = components["schemas"]["ContactReason"];

/** One contact an import run created or changed, and why it is listed. */
type ImportContactRow = {
  id: number;
  name: string;
  reason: ContactReason;
};

/** The reason as the person reads it. */
const REASON_LABEL: Record<ContactReason, string> = {
  created: "New",
  replaced_trashed: "New, replaces a trashed contact",
  named: "Named",
  handle_added: "Handle added",
};

/** One page of them, as every list route answers. */
type ImportContactsPage = {
  items: ImportContactRow[];
  total: number;
  limit: number;
  offset: number;
};

/** A contact the run learned an address for but no name yet. */
const UNNAMED = "(unknown)";

/**
 * The contacts one import run created or changed, each with what the run did
 * to it.
 *
 * A run creates a contact for every participant it meets, so this is where a
 * person sees who arrived with a given backup, and which of them came back
 * from the Trash. Contacts with no name yet are the ones waiting in the
 * Unknown group.
 */
export default function ImportContactsPanel({
  importId,
  newCount,
  changedCount,
}: {
  importId: number;
  newCount: number;
  changedCount: number;
}) {
  const [data, setData] = useState<ImportContactsPage | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    getImportContacts(importId)
      .then((res) => {
        if (!cancelled) setData(res);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Could not load contacts for this import.");
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [importId]);

  if (loading) return <div className="text-[0.813rem] text-muted">Loading contacts…</div>;
  if (error) return <div className="text-[0.813rem] text-danger">{error}</div>;
  if (!data || data.items.length === 0) {
    return <div className="text-[0.813rem] text-muted">This import changed no contacts.</div>;
  }

  return (
    <div>
      <p className="mb-2 text-[0.813rem] text-muted">
        {newCount.toLocaleString()} new, {changedCount.toLocaleString()} changed
      </p>
      <ul className="max-h-48 overflow-y-auto text-[0.813rem]">
        {data.items.map((c) => (
          <li key={c.id} className="flex items-center justify-between gap-3 py-0.5">
            <span className={c.name.trim() ? "truncate" : "truncate text-muted"}>
              {c.name.trim() || UNNAMED}
            </span>
            <span className="shrink-0 text-muted">{REASON_LABEL[c.reason]}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
