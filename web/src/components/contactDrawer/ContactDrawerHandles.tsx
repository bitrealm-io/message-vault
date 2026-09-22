import { type ReactNode, useMemo } from "react";
import type { ContactDetail, ContactHandle } from "../../lib/contactDetail";
import AddIdentityDialog from "../AddIdentityDialog";
import Button from "../Button";
import ConfirmDialog from "../ConfirmDialog";
import DataCard from "../DataCard";
import IdentityTable, { type IdentityRow } from "../IdentityTable";
import type { ContactBrowseKind } from "./contactDrawerTypes";
import { conversationCount, removeIdentityConfirmBody } from "./handleTableLogic";
import { useHandleMutations } from "./useHandleMutations";

type BrowseFn = (args: { kind: ContactBrowseKind; handle?: string }) => void;

/** A contact's identity as the shared table shows it. */
function toIdentityRow(h: ContactHandle): IdentityRow {
  return {
    handle: h.handle,
    service: h.service ?? null,
    start_date: h.start_date ?? null,
    end_date: h.end_date ?? null,
    conversations: conversationCount(h),
    direct_messages: h.individual_message_count,
    group_messages: h.group_message_count,
  };
}

/**
 * The identities of the contact in the drawer, with what each takes part in,
 * and the way to add one or remove one. The conversation counts lead to the
 * conversation list, and the Summary row adds every column up.
 */
export function ContactDrawerHandles({
  contactId,
  handleRows,
  loading,
  onBrowse,
  title = "Contact Identity",
  intro,
  toolbarExtra,
}: {
  contactId: string;
  handleRows: ContactDetail["handles"];
  loading: boolean;
  onBrowse?: BrowseFn;
  title?: ReactNode;
  intro?: ReactNode;
  toolbarExtra?: ReactNode;
}) {
  const {
    adding,
    setAdding,
    busy,
    error: mutationError,
    removeTarget,
    setRemoveTarget,
    requestRemoveHandle,
    confirmRemoveHandle,
    confirmAdd,
  } = useHandleMutations({ contactId });

  const rows = useMemo(() => handleRows.map(toIdentityRow), [handleRows]);

  const requestRemove = (row: IdentityRow) => {
    const original = handleRows.find(
      (h) => h.handle === row.handle && (h.service ?? null) === row.service,
    );
    if (original) requestRemoveHandle(original);
  };

  return (
    <DataCard
      title={title}
      intro={intro}
      toolbar={toolbarExtra}
      bodyClassName="min-w-0 overflow-x-auto"
    >
      <div className="mb-2 flex justify-end">
        <Button
          variant="primary"
          disabled={loading || busy}
          onClick={() => setAdding(true)}
          size="chip"
        >
          Add identity
        </Button>
      </div>
      {/* Keyed on the contact so the sort order starts over with each one. */}
      <IdentityTable
        key={contactId}
        ariaLabel="Contact identities"
        rows={rows}
        loading={loading}
        busy={busy}
        totals
        emptyText={loading ? "Loading…" : "No identities"}
        onRemove={requestRemove}
        onBrowse={onBrowse ? (row) => onBrowse({ kind: "all", handle: row.handle }) : undefined}
      />
      <AddIdentityDialog
        open={adding}
        busy={busy}
        error={mutationError}
        existing={handleRows}
        onClose={() => {
          if (!busy) setAdding(false);
        }}
        onConfirm={(args) => void confirmAdd(args)}
      />
      <ConfirmDialog
        open={removeTarget !== null}
        title="Remove identity from contact?"
        body={removeTarget ? removeIdentityConfirmBody(removeTarget) : null}
        confirmLabel="Remove identity"
        danger
        busy={busy}
        error={mutationError}
        onClose={() => {
          if (!busy) setRemoveTarget(null);
        }}
        onConfirm={() => void confirmRemoveHandle()}
      />
    </DataCard>
  );
}
