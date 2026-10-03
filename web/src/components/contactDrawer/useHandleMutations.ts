import { useState } from "react";
import { type ContactHandle, useUpdateContact } from "../../lib/contactDetail";
import { formatHandleServiceLabel, inferService } from "./contactDrawerTypes";
import type { RemoveIdentityTarget } from "./handleTableLogic";

/**
 * The add and remove dialogs of a contact's handle table, and the update they send.
 * `ContactDrawer` keys the table by contact id, so this state starts fresh for each contact.
 */
export function useHandleMutations({ contactId }: { contactId: string }) {
  const [adding, setAdding] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<RemoveIdentityTarget | null>(null);
  const updateContact = useUpdateContact();
  const busy = updateContact.isPending;
  // The dialogs stay open on a refusal and show this, so a person can retry.
  const error = updateContact.error ? updateContact.error.message : "";

  const requestRemoveHandle = (h: ContactHandle) => {
    if (busy) return;
    setRemoveTarget({
      address: h.address,
      service: h.service ?? null,
      serviceLabel: formatHandleServiceLabel(h.address, h.service),
      conversationCount: h.conversations,
    });
  };

  const confirmRemoveHandle = () => {
    if (!removeTarget || busy) return;
    const address = removeTarget.address;
    const service = inferService(address, removeTarget.service);
    updateContact.mutate(
      { contactId, body: { remove_identity: { address, service } } },
      { onSuccess: () => setRemoveTarget(null) },
    );
  };

  const confirmAdd = (args: { address: string; service: string }) => {
    if (busy) return;
    updateContact.mutate(
      { contactId, body: { add_identity: { address: args.address, service: args.service } } },
      { onSuccess: () => setAdding(false) },
    );
  };

  return {
    adding,
    setAdding,
    busy,
    error,
    removeTarget,
    setRemoveTarget,
    requestRemoveHandle,
    confirmRemoveHandle,
    confirmAdd,
  };
}
