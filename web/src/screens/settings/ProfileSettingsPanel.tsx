import { useState } from "react";
import Button from "../../components/Button";
import TimeZoneField from "../../components/TimeZoneField";
import { useSettingsAccount, useUpdateSettingsProfile } from "../../lib/useSettingsAccount";
import { AccountActivitySection } from "./AccountActivitySection";
import { AddressBookSection } from "./AddressBookSection";
import { IdentitiesSection } from "./IdentitiesSection";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Profile settings: display name, time zone, identities, address book.
 *
 * Given `managedAccountId`, the account is one the owner opened from
 * User Accounts. The owner sets its name, zone and identities as the holder
 * does, and reads when it last logged in and which app it connects with. The
 * address book is the account's contacts, which the owner does not reach.
 */
export function ProfileSettingsPanel({ managedAccountId }: { managedAccountId?: number }) {
  const { profile, loading, error: loadError } = useSettingsAccount(managedAccountId);
  const updateProfile = useUpdateSettingsProfile(managedAccountId);
  const managed = managedAccountId !== undefined;
  // What the person has typed and not yet saved; null shows the stored name.
  const [nameDraft, setNameDraft] = useState<string | null>(null);
  const [nameError, setNameError] = useState("");
  const [zoneError, setZoneError] = useState("");

  if (loadError) {
    return <div className="text-danger">Could not load profile: {loadError}</div>;
  }

  if (loading || !profile) {
    return <div className="text-muted">Loading…</div>;
  }

  const name = nameDraft ?? profile.preferred_name ?? "";

  const handleSaveName = async () => {
    setNameError("");
    try {
      await updateProfile.mutateAsync({
        preferred_name: name.trim() || null,
      });
      setNameDraft(null);
    } catch (e) {
      setNameError(e instanceof Error ? e.message : String(e));
    }
  };

  // Picking a zone is the whole gesture: no Save button, the server answers
  // with the profile as it now stands and every date label re-renders from it.
  const handleChangeZone = async (zone: string) => {
    if (zone === profile.time_zone) return;
    setZoneError("");
    try {
      await updateProfile.mutateAsync({ time_zone: zone });
    } catch (e) {
      setZoneError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div>
      <h3 className={sectionTitleClass}>Display Name</h3>
      <div className="mb-[0.35rem] flex items-center gap-2">
        <input
          type="text"
          aria-label="Display name"
          value={name}
          onChange={(e) => setNameDraft(e.target.value)}
          className={`${inputClassName} flex-1`}
        />
        <Button variant="primary" onClick={handleSaveName} className="!px-[0.85rem] !py-[0.35rem]">
          Save
        </Button>
      </div>
      {nameError && <div className="mb-6 text-[0.813rem] text-danger">{nameError}</div>}
      {!nameError && <div className="mb-6" />}

      <h3 className={sectionTitleClass}>Time Zone</h3>
      <TimeZoneField
        value={profile.time_zone}
        onChange={(zone) => void handleChangeZone(zone)}
        isDisabled={updateProfile.isPending}
        className="mb-[0.35rem] max-w-[28rem]"
      />
      <div className="text-[0.813rem] text-muted">Message times are shown in this zone.</div>
      {zoneError && <div className="mb-6 text-[0.813rem] text-danger">{zoneError}</div>}
      {!zoneError && <div className="mb-6" />}

      {/* The owner holds no messages: it has no handles to match a sender
          against and no contacts, so neither section is its to fill in. */}
      {profile.is_owner ? null : (
        <>
          <IdentitiesSection profile={profile} managedAccountId={managedAccountId} />
          {managed ? <AccountActivitySection profile={profile} /> : <AddressBookSection />}
        </>
      )}
    </div>
  );
}
