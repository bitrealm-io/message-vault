import { useMemo, useState } from "react";
import AddIdentityDialog from "../../components/AddIdentityDialog";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import IdentityTable, { type IdentityRow } from "../../components/IdentityTable";
import type { AccountProfile } from "../../lib/account";
import type { HandleService } from "../../lib/handleService";
import { phonesMatch } from "../../lib/phoneTokens";
import { useUpdateSettingsProfile } from "../../lib/useSettingsAccount";
import { listAccountIdentities } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";
import { type Identity, removeBody } from "./identities";
import { sectionTitleClass } from "./profileStyles";

/** Whether `profile` lists `handle` on `service`, however either was typed. */
function profileIncludes(p: AccountProfile, handle: string, service: string): boolean {
  const needle = handle.trim().toLowerCase();
  if (service === "email") {
    return p.emails.some((e) => e.toLowerCase() === needle);
  }
  // Phone and WhatsApp both come back in profile.phones (E.164 when unambiguous).
  return p.phones.some((phone) => phonesMatch(handle, phone));
}

/** The profile's own identities as placeholder rows, shown until the vault lists them. */
function placeholderRows(profile: AccountProfile): Identity[] {
  return [
    ...profile.phones.map((address) => ({ address, service: "phone" })),
    ...profile.emails.map((address) => ({ address, service: "email" })),
  ].map((row) => ({
    ...row,
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
  }));
}

/**
 * The account's own identities: the addresses whose messages are the account
 * holder's. Import uses them to decide which messages belong to the holder.
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
  const managed = managedAccountId !== undefined;
  const updateProfile = useUpdateSettingsProfile(managedAccountId);
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState("");
  const [removeTarget, setRemoveTarget] = useState<Identity | null>(null);
  const [removeError, setRemoveError] = useState("");
  const busy = updateProfile.isPending;

  // Until the vault answers, the profile's own identities are shown with no
  // counts, so the table never waits on a fetch and never shows a number
  // that is not the vault's.
  const identities = useVaultQuery(
    managed ? keys.ownerAccounts.identities(managedAccountId) : keys.accountProfile.identities,
    (signal) => listAccountIdentities({ signal }, managedAccountId),
  );
  const listed = identities.data?.items;
  const rows: Identity[] = useMemo(() => listed ?? placeholderRows(profile), [listed, profile]);
  const tableRows: IdentityRow[] = useMemo(
    () =>
      rows.map((row) => ({
        ...row,
        start_date: row.start_date ?? null,
        end_date: row.end_date ?? null,
      })),
    [rows],
  );

  const confirmAdd = async ({ address, service }: { address: string; service: HandleService }) => {
    setAddError("");
    try {
      const updated = await updateProfile.mutateAsync({ identities: [{ address, service }] });
      if (!profileIncludes(updated, address, service)) {
        throw new Error("The vault did not add that identity.");
      }
      setAdding(false);
    } catch (e) {
      setAddError(e instanceof Error ? e.message : String(e));
    }
  };

  const confirmRemove = async () => {
    if (!removeTarget) return;
    const { address, service } = removeTarget;
    setRemoveError("");
    try {
      const updated = await updateProfile.mutateAsync({
        remove_identities: [{ address, service }],
      });
      if (profileIncludes(updated, address, service)) {
        throw new Error("The vault did not remove that identity.");
      }
      setRemoveTarget(null);
    } catch (e) {
      setRemoveError(e instanceof Error ? e.message : String(e));
    }
  };

  const requestRemove = (row: IdentityRow) => {
    const target = rows.find((r) => r.address === row.address && r.service === row.service);
    if (target) {
      setRemoveError("");
      setRemoveTarget(target);
    }
  };

  return (
    <>
      <h3 className={sectionTitleClass}>{managed ? "Identities" : "My Identities"}</h3>
      <p className="mt-0 mb-3 text-[0.813rem] text-muted">
        {managed
          ? "The account holder's phone numbers and emails. Import uses them to determine which messages belong to them."
          : "Your phone numbers and emails. Import uses them to determine which messages belong to you."}
      </p>
      <div className="overflow-x-auto">
        <IdentityTable
          ariaLabel="Identities"
          rows={tableRows}
          loading={listed === undefined}
          busy={busy}
          emptyText="No identities yet."
          onRemove={requestRemove}
        />
      </div>
      <div className="mt-3 mb-6">
        <Button
          variant="primary"
          size="sm"
          isDisabled={busy}
          onPress={() => {
            setAddError("");
            setAdding(true);
          }}
        >
          Add identity
        </Button>
      </div>

      <AddIdentityDialog
        open={adding}
        busy={busy}
        error={addError}
        existing={rows}
        onClose={() => {
          if (!busy) setAdding(false);
        }}
        onConfirm={(args) => void confirmAdd(args)}
      />
      <ConfirmDialog
        open={removeTarget !== null}
        title="Remove identity?"
        body={removeTarget ? removeBody(removeTarget) : null}
        confirmLabel="Remove"
        danger
        busy={busy}
        error={removeTarget ? removeError : ""}
        onClose={() => {
          if (!busy) setRemoveTarget(null);
        }}
        onConfirm={() => void confirmRemove()}
      />
    </>
  );
}
