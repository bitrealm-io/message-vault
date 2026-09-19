import { useSettingsAccount } from "../../lib/useSettingsAccount";
import { ApiTokensSection } from "./ApiTokensSection";
import { ChangePasswordSection } from "./ChangePasswordSection";
import { ProfileDangerZone } from "./ProfileDangerZone";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Account settings: username, password, API tokens, danger zone.
 *
 * Given `managedAccountId`, the account is one the vault owner opened from
 * User Accounts. API tokens are the account holder's own to mint and see, so
 * the owner is not shown them. The vault owner's own account has no tokens and
 * cannot be deleted, so it has neither section.
 */
export function AccountSettingsPanel({ managedAccountId }: { managedAccountId?: number }) {
  const { profile, loading, error: loadError } = useSettingsAccount(managedAccountId);

  if (loadError) {
    return <div className="text-danger">Could not load account: {loadError}</div>;
  }

  if (loading || !profile) {
    return <div className="text-muted">Loading…</div>;
  }

  const managed = managedAccountId !== undefined;
  const isOwner = profile.is_owner === true;
  // The demo lock is the account's own; the owner may set the demo account's password.
  const isDemo = profile.is_demo === true && !managed;

  return (
    <div>
      <h3 className={sectionTitleClass}>Username</h3>
      <div className="mb-6 max-w-[360px]">
        <input
          type="text"
          value={profile.username}
          readOnly
          className={`${inputClassName} !text-muted`}
        />
      </div>
      <ChangePasswordSection
        disabled={isDemo}
        canReset={!isOwner}
        requireCurrent={isOwner && !managed}
        managedAccountId={managedAccountId}
      />

      {!managed && !isOwner ? (
        <ApiTokensSection
          accountCanImport={profile.can_import ?? true}
          accountCanExport={profile.can_export ?? true}
          accountCanDelete={profile.can_delete ?? false}
        />
      ) : null}

      {!isOwner ? (
        <ProfileDangerZone
          isDemo={profile.is_demo === true}
          username={profile.username}
          managedAccountId={managedAccountId}
          messageCount={profile.message_count}
        />
      ) : null}
    </div>
  );
}
