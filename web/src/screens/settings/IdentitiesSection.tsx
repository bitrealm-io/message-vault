import { useMemo, useState } from "react";
import {
  Cell,
  Column,
  Row,
  type SortDescriptor,
  Table,
  TableBody,
  TableHeader,
} from "react-aria-components";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import type { AccountProfile } from "../../lib/account";
import {
  formatHandleServiceLabel,
  HANDLE_SERVICE_OPTIONS,
  HANDLE_SERVICES,
  type HandleService,
  handlePlaceholder,
  handleValidationError,
} from "../../lib/handleService";
import { phonesMatch } from "../../lib/phoneTokens";
import { parseSelectKey } from "../../lib/selectKey";
import { useUpdateSettingsProfile } from "../../lib/useSettingsAccount";
import { listAccountIdentities } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";
import { type Identity, inUse, removeBody, SORT_COLUMNS, sortIdentities } from "./identities";
import { inputClassName, sectionTitleClass } from "./profileStyles";

const thClass =
  "py-1.5 pr-6 text-left text-[0.688rem] font-semibold uppercase tracking-[0.04em] text-muted outline-none cursor-pointer hover:text-accent data-hovered:text-accent";
const tdClass = "py-1.5 pr-6 align-middle text-[0.875rem] text-text";

/**
 * The phone numbers and email addresses that are this account's own, which is
 * how the vault tells the messages it sent from the ones it received. The
 * vault calls them handles; the screen calls them identities.
 *
 * Given `managedAccountId`, they are an account's the vault owner opened from
 * User Accounts, and the owner adds and removes them as the holder does.
 */
export function IdentitiesSection({
  profile,
  managedAccountId,
}: {
  profile: AccountProfile;
  managedAccountId?: number;
}) {
  const updateProfile = useUpdateSettingsProfile(managedAccountId);
  const [newHandle, setNewHandle] = useState("");
  const [newHandleService, setNewHandleService] = useState<HandleService>("phone");
  const [handleError, setHandleError] = useState("");
  const [removeTarget, setRemoveTarget] = useState<Identity | null>(null);
  const [sortDescriptor, setSortDescriptor] = useState<SortDescriptor | null>(null);
  const handleBusy = updateProfile.isPending;

  const handleListIncludes = (p: AccountProfile, handle: string, service: string) => {
    const needle = handle.trim().toLowerCase();
    if (service === "email") {
      return p.emails.some((e) => e.toLowerCase() === needle);
    }
    // Phone and WhatsApp both come back in profile.phones (E.164 when unambiguous).
    return p.phones.some((phone) => phonesMatch(handle, phone));
  };

  const handleAddHandle = async () => {
    const value = newHandle.trim();
    if (!value) return;
    // The same check Create Vault Owner runs on its identities, so a number
    // with too few digits or an address without an @ never reaches the vault.
    const invalid = handleValidationError(newHandleService, value);
    if (invalid) {
      setHandleError(invalid);
      return;
    }
    setHandleError("");
    try {
      const updated = await updateProfile.mutateAsync({
        handles: [{ handle: value, service: newHandleService }],
      });
      if (!handleListIncludes(updated, value, newHandleService)) {
        throw new Error("The vault did not add that identity.");
      }
      setNewHandle("");
    } catch (e) {
      setHandleError(e instanceof Error ? e.message : String(e));
    }
  };

  const confirmRemove = async () => {
    if (!removeTarget) return;
    const { handle, service } = removeTarget;
    setHandleError("");
    try {
      const updated = await updateProfile.mutateAsync({
        remove_handles: [{ handle, service }],
      });
      if (handleListIncludes(updated, handle, service)) {
        throw new Error("The vault did not remove that identity.");
      }
      setRemoveTarget(null);
    } catch (e) {
      setHandleError(e instanceof Error ? e.message : String(e));
    }
  };

  // The counts come from the vault's identities list. Until it answers, the
  // profile's own identities are shown with no counts, so the table never
  // waits on a fetch and never shows a number that is not the vault's.
  const identities = useVaultQuery(
    managedAccountId === undefined
      ? keys.accountProfile.identities
      : keys.ownerAccounts.identities(managedAccountId),
    (signal) => listAccountIdentities({ signal }, managedAccountId),
  );
  const rows = useMemo(() => {
    const listed = identities.data?.items;
    const all: Identity[] = listed
      ? listed
      : [
          ...profile.phones.map((handle) => ({ handle, service: "phone" })),
          ...profile.emails.map((handle) => ({ handle, service: "email" })),
        ].map((row) => ({ ...row, direct_messages: 0, group_messages: 0 }));
    const column = parseSelectKey(sortDescriptor?.column ?? null, SORT_COLUMNS);
    return sortIdentities(
      all,
      column && sortDescriptor ? { column, direction: sortDescriptor.direction } : null,
    );
  }, [identities.data, profile.phones, profile.emails, sortDescriptor]);
  const counted = identities.data !== undefined;

  const sortGlyph = (sortDirection: "ascending" | "descending" | undefined) => (
    <span
      aria-hidden="true"
      className={`ml-1 text-[0.55rem] leading-none ${sortDirection ? "text-accent" : "invisible"}`}
    >
      {sortDirection === "descending" ? "▼" : "▲"}
    </span>
  );

  return (
    <>
      <h3 className={sectionTitleClass}>
        {managedAccountId === undefined ? "My Identities" : "Identities"}
      </h3>
      {rows.length === 0 ? (
        <div className="mb-3 text-[0.875rem] text-muted">None</div>
      ) : (
        <Table
          aria-label="Identities"
          className="mb-3 w-full max-w-[36rem] border-collapse"
          sortDescriptor={sortDescriptor ?? undefined}
          onSortChange={setSortDescriptor}
        >
          <TableHeader className="border-b border-border">
            <Column id="service" allowsSorting className={`${thClass} w-[9rem]`}>
              {({ sortDirection }) => (
                <>
                  Type
                  {sortGlyph(sortDirection)}
                </>
              )}
            </Column>
            <Column id="handle" isRowHeader allowsSorting className={thClass}>
              {({ sortDirection }) => (
                <>
                  Identity
                  {sortGlyph(sortDirection)}
                </>
              )}
            </Column>
            <Column id="in_use" allowsSorting className={thClass}>
              {({ sortDirection }) => (
                <>
                  In use
                  {sortGlyph(sortDirection)}
                </>
              )}
            </Column>
            {/* Remove has no heading: each button is labelled with its identity. */}
            <Column className="w-16" aria-label="Actions">
              {""}
            </Column>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <Row
                key={`${row.service}-${row.handle}`}
                id={`${row.service}-${row.handle}`}
                className="group border-b border-border outline-none"
              >
                <Cell className={`${tdClass} text-muted`}>
                  {formatHandleServiceLabel(row.handle, row.service)}
                </Cell>
                <Cell className={tdClass}>{row.handle}</Cell>
                <Cell className={`${tdClass} text-muted`}>
                  {counted ? (inUse(row) ? "Yes" : "No") : "—"}
                </Cell>
                <Cell className="py-0.5 pr-0 text-right align-middle">
                  {/* Shown when the pointer is on its row; the dialog it opens names the messages. */}
                  <Button
                    variant="ghost"
                    onClick={() => setRemoveTarget(row)}
                    disabled={handleBusy}
                    aria-label={`Remove ${row.handle}`}
                    className="!px-2 !py-[0.2rem] !text-[0.813rem] !text-danger opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  >
                    Remove
                  </Button>
                </Cell>
              </Row>
            ))}
          </TableBody>
        </Table>
      )}

      <div className="mb-[0.35rem] flex flex-wrap items-center gap-2">
        {/* Wide enough for "Text message", the longest type, on one line, and
            no wider or narrower whichever type is picked. */}
        <Select
          selectedKey={newHandleService}
          onSelectionChange={(k) => {
            const service = parseSelectKey(k, HANDLE_SERVICES);
            if (service) setNewHandleService(service);
          }}
          aria-label="Identity service"
          className="w-[10.5rem] shrink-0"
        >
          {HANDLE_SERVICE_OPTIONS.map((s) => (
            <ListBoxItem key={s.value} id={s.value} className={selectItemClassName}>
              {s.label}
            </ListBoxItem>
          ))}
        </Select>
        <input
          type="text"
          value={newHandle}
          onChange={(e) => setNewHandle(e.target.value)}
          placeholder={handlePlaceholder(newHandleService)}
          aria-label="New identity"
          className={`${inputClassName} min-w-[12rem] flex-1`}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void handleAddHandle();
            }
          }}
        />
        <Button
          variant="primary"
          onClick={handleAddHandle}
          disabled={handleBusy || !newHandle.trim()}
          className="!px-[0.85rem] !py-[0.35rem]"
        >
          Add
        </Button>
      </div>
      {handleError && <div className="mb-6 text-[0.813rem] text-danger">{handleError}</div>}
      {!handleError && <div className="mb-6" />}

      <ConfirmDialog
        open={removeTarget !== null}
        title="Remove identity?"
        body={removeTarget ? removeBody(removeTarget) : null}
        confirmLabel="Remove"
        danger
        busy={handleBusy}
        error={removeTarget ? handleError : ""}
        onClose={() => {
          if (!handleBusy) setRemoveTarget(null);
        }}
        onConfirm={() => void confirmRemove()}
      />
    </>
  );
}
