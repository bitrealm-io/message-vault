import { useState } from "react";
import Button from "./Button";
import ModalShell, { DialogError } from "./ModalShell";

interface SavedSearchFormProps {
  onSave: (name: string, query: string) => void | Promise<void>;
  onCancel: () => void;
  initial?: { name: string; query: string };
  /** Why the last save was refused. The form stays open so it can be corrected. */
  error?: string | null;
  busy?: boolean;
}

export default function SavedSearchForm({
  onSave,
  onCancel,
  initial,
  error = null,
  busy = false,
}: SavedSearchFormProps) {
  const [name, setName] = useState(initial?.name || "");
  const [query, setQuery] = useState(initial?.query || "");

  const handleSave = () => {
    if (!name.trim() || !query.trim() || busy) return;
    void onSave(name.trim(), query.trim());
  };

  return (
    <ModalShell
      open
      onOpenChange={(o) => {
        if (!o) onCancel();
      }}
      label={initial ? "Edit saved search" : "New saved search"}
      maxWidth="25rem"
    >
      <h3 className="mb-4 text-[1rem] text-text">
        {initial ? "Edit saved search" : "New saved search"}
      </h3>

      <label className="mb-3 block">
        <span className="mb-1 block text-[0.813rem] font-medium text-text">Name</span>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSave()}
          placeholder="e.g. Work team"
          className="box-border w-full rounded border border-border bg-elevated px-2 py-1.5 text-[0.875rem] text-text"
        />
      </label>

      <label className="block">
        <span className="mb-1 block text-[0.813rem] font-medium text-text">Query</span>
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSave()}
          placeholder="e.g. service:whatsapp kind:group"
          className="box-border w-full rounded border border-border bg-elevated px-2 py-1.5 text-[0.875rem] text-text"
        />
      </label>

      <DialogError message={error ?? ""} />

      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={onCancel} disabled={busy} size="sm">
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={handleSave}
          disabled={busy || !name.trim() || !query.trim()}
          size="sm"
          className="!px-4"
        >
          Save
        </Button>
      </div>
    </ModalShell>
  );
}
