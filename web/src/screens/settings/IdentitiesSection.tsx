import { useState } from "react";
import Button from "../../components/Button";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import type { AccountProfile } from "../../lib/account";
import {
  HANDLE_SERVICE_OPTIONS,
  HANDLE_SERVICES,
  type HandleService,
  handlePlaceholder,
} from "../../lib/handleService";
import { phonesMatch } from "../../lib/phoneTokens";
import { parseSelectKey } from "../../lib/selectKey";
import { useUpdateSettingsProfile } from "../../lib/useSettingsAccount";
import { inputClassName, sectionTitleClass } from "./profileStyles";

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

  const handleRemoveHandle = async (handle: string, service: string) => {
    setHandleError("");
    try {
      const updated = await updateProfile.mutateAsync({
        remove_handles: [{ handle, service }],
      });
      if (handleListIncludes(updated, handle, service)) {
        throw new Error("The vault did not remove that identity.");
      }
    } catch (e) {
      setHandleError(e instanceof Error ? e.message : String(e));
    }
  };

  const handles = [
    ...profile.phones.map((handle) => ({ handle, service: "phone" })),
    ...profile.emails.map((handle) => ({ handle, service: "email" })),
  ];

  return (
    <>
      <h3 className={sectionTitleClass}>
        {managedAccountId === undefined ? "My Identities" : "Identities"}
      </h3>
      {handles.length === 0 ? (
        <div className="mb-3 text-[0.875rem] text-muted">None</div>
      ) : (
        <div className="mb-3">
          {handles.map((h) => (
            <div
              key={`${h.service}-${h.handle}`}
              className="flex items-center gap-3 border-b border-border py-1.5 text-[0.875rem]"
            >
              <span className="min-w-[7rem] shrink-0 text-muted">{h.service}</span>
              <span className="min-w-0 flex-1">{h.handle}</span>
              <Button
                variant="ghost"
                onClick={() => handleRemoveHandle(h.handle, h.service)}
                disabled={handleBusy}
                className="!px-2 !py-[0.2rem] !text-[0.813rem] !text-danger"
              >
                Remove
              </Button>
            </div>
          ))}
        </div>
      )}

      <div className="mb-[0.35rem] flex flex-wrap items-center gap-2">
        <Select
          selectedKey={newHandleService}
          onSelectionChange={(k) => {
            const service = parseSelectKey(k, HANDLE_SERVICES);
            if (service) setNewHandleService(service);
          }}
          aria-label="Identity service"
          className="shrink-0 min-w-[7rem]"
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
    </>
  );
}
