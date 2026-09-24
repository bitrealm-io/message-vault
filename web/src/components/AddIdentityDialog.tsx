import { useEffect, useId, useState } from "react";
import {
  HANDLE_SERVICE_OPTIONS,
  HANDLE_SERVICES,
  type HandleService,
  handleDuplicateKey,
  handlePlaceholder,
  handleValidationError,
  inferService,
} from "../lib/handleService";
import { parseSelectKey } from "../lib/selectKey";
import { Z_POPOVER_IN_MODAL } from "../lib/zLayers";
import Button from "./Button";
import ModalShell, { DialogError, DialogFooter } from "./ModalShell";
import Select, { ListBoxItem, selectItemClassName } from "./Select";

const fieldLabelClass = "mb-1 block text-[0.813rem] font-medium text-text";
const inputClass =
  "box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] font-normal leading-none text-text outline-none focus:border-accent";
const selectTriggerClass =
  "!box-border !h-9 !min-h-9 !w-full !rounded !px-3 !py-0 !text-[0.875rem] !font-normal !leading-none !bg-elevated";
const selectValueClass = "!text-[0.875rem] !font-normal !leading-none";

const DUPLICATE_MESSAGE = "This identity is already in the list.";

/** Whether `existing` already holds `handle` on `service`, however it was typed. */
function alreadyListed(
  existing: readonly { address: string; service?: string | null }[],
  handle: string,
  service: HandleService,
): boolean {
  const key = handleDuplicateKey(service, handle);
  if (!key) return false;
  return existing.some((row) => {
    const rowService = inferService(row.address, row.service);
    const known = HANDLE_SERVICES.find((s) => s === rowService) ?? "phone";
    return handleDuplicateKey(known, row.address) === key;
  });
}

/**
 * Adding one identity to a contact or to an account: the service it is on
 * and the address. The contact drawer and the Profile tab open the same
 * dialog, so an identity is added the same way wherever it is added.
 *
 * A value that is not a number or an address, or one already in `existing`
 * on the same service, is refused here before anyone is asked. The same
 * number on Text message and on WhatsApp is two identities, so it is not a
 * duplicate.
 */
export default function AddIdentityDialog({
  open,
  busy = false,
  error = "",
  existing = [],
  onClose,
  onConfirm,
}: {
  open: boolean;
  busy?: boolean;
  /** Why the last submit failed. The dialog stays open so it can be retried. */
  error?: string;
  existing?: readonly { address: string; service?: string | null }[];
  onClose: () => void;
  onConfirm: (args: { address: string; service: HandleService }) => void;
}) {
  const [service, setService] = useState<HandleService>("phone");
  const [handle, setHandle] = useState("");
  const [invalid, setInvalid] = useState("");
  const serviceId = useId();
  const problemId = useId();

  useEffect(() => {
    if (open) {
      setService("phone");
      setHandle("");
      setInvalid("");
    }
  }, [open]);

  const trimmed = handle.trim();
  const duplicate = alreadyListed(existing, handle, service);
  const problem = duplicate ? DUPLICATE_MESSAGE : invalid;
  const canSubmit = trimmed.length > 0 && !duplicate && !busy;

  const submit = () => {
    if (!canSubmit) return;
    const why = handleValidationError(service, trimmed);
    if (why) {
      setInvalid(why);
      return;
    }
    onConfirm({ address: trimmed, service });
  };

  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      dismissable={!busy}
      label="Add identity"
      title="Add identity"
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="24rem"
    >
      <div className="mt-4 mb-4">
        <label htmlFor={serviceId} className={fieldLabelClass}>
          Service
        </label>
        <Select
          id={serviceId}
          selectedKey={service}
          onSelectionChange={(k) => {
            const next = parseSelectKey(k, HANDLE_SERVICES);
            if (next) {
              setService(next);
              setInvalid("");
            }
          }}
          aria-label="Service"
          isDisabled={busy}
          triggerClassName={selectTriggerClass}
          valueClassName={selectValueClass}
          popoverClassName={Z_POPOVER_IN_MODAL}
          className="block w-full min-w-0"
        >
          {HANDLE_SERVICE_OPTIONS.map((s) => (
            <ListBoxItem key={s.value} id={s.value} className={selectItemClassName}>
              {s.label}
            </ListBoxItem>
          ))}
        </Select>
      </div>

      <label className="block">
        <span className={fieldLabelClass}>Identity</span>
        <input
          type="text"
          value={handle}
          onChange={(e) => {
            setHandle(e.target.value);
            setInvalid("");
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit();
            }
          }}
          disabled={busy}
          autoComplete="off"
          spellCheck={false}
          placeholder={handlePlaceholder(service)}
          aria-invalid={problem ? true : undefined}
          aria-describedby={problem ? problemId : undefined}
          className={inputClass}
        />
      </label>
      {problem ? (
        <p id={problemId} className="mt-2 text-[0.813rem] text-danger" role="alert">
          {problem}
        </p>
      ) : null}

      <DialogError message={error} />

      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" onPress={submit} isDisabled={!canSubmit}>
          {busy ? "Working…" : "Add"}
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}
