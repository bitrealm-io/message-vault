import { useEffect, useState } from "react";
import { type ContactHandle, useUpdateContact } from "../../lib/contactDetail";
import { formatHandleServiceLabel, inferService } from "./contactDrawerTypes";
import type { RemoveIdentityTarget } from "./handleTableLogic";

export function useHandleMutations({ contactId }: { contactId: string }) {
  const [adding, setAdding] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<RemoveIdentityTarget | null>(null);
  const updateContact = useUpdateContact();
  const busy = updateContact.isPending;
  // The dialogs stay open on a refusal and show this, so a person can retry.
  const error = updateContact.error ? updateContact.error.message : "";
  const reset = updateContact.reset;

  useEffect(() => {
    void contactId;
    setAdding(false);
    setRemoveTarget(null);
    reset();
  }, [contactId, reset]);

  const requestRemoveHandle = (h: ContactHandle) => {
    if (busy) return;
    setRemoveTarget({
      handle: h.handle,
      service: h.service ?? null,
      serviceLabel: formatHandleServiceLabel(h.handle, h.service),
      conversationCount: h.conversations,
    });
  };

  const confirmRemoveHandle = () => {
    if (!removeTarget || busy) return;
    const handle = removeTarget.handle;
    const service = inferService(handle, removeTarget.service);
    updateContact.mutate(
      { contactId, body: { remove_handle: { handle, service } } },
      { onSuccess: () => setRemoveTarget(null) },
    );
  };

  const confirmAdd = (args: { handle: string; service: string }) => {
    if (busy) return;
    updateContact.mutate(
      { contactId, body: { add_handle: { handle: args.handle, service: args.service } } },
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
