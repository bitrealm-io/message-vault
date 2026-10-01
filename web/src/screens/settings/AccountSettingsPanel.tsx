import { useSettingsAccount } from "../../lib/useSettingsAccount";
import { AccountPermissionsSection } from "./AccountPermissionsSection";
import { ApiTokensSection } from "./ApiTokensSection";
import { ChangePasswordSection } from "./ChangePasswordSection";
import { ProfileDangerZone } from "./ProfileDangerZone";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Account settings: username, password, status, permissions, API tokens,
 * danger zone.
 *
 * Given `managedAccountId`, the account is one the owner opened from
 * User Accounts. API tokens are the account holder's own to mint and see, so
 * the owner is not shown them. The owner's own account has no tokens and
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
  // The Demo Account never has a password, whoever asks: anyone at the login
  // card enters it (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
  const isDemo = profile.is_demo === true;

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
      {/* The owner cannot be disabled and holds no messages to import, export or delete. */}
      {!isOwner ? (
        <AccountPermissionsSection profile={profile} managedAccountId={managedAccountId} />
      ) : null}

      {isDemo ? (
        <>
          <h3 className={sectionTitleClass}>Password</h3>
          <p className="mb-6 mt-0 text-[0.813rem] text-muted">
            The Demo Account never has a password.
          </p>
        </>
      ) : (
        <ChangePasswordSection
          canReset={!isOwner}
          requireCurrent={isOwner && !managed}
          managedAccountId={managedAccountId}
        />
      )}

      {!managed && !isOwner ? (
        <ApiTokensSection
          accountCanImport={profile.can_import ?? true}
          accountCanExport={profile.can_export ?? true}
        />
      ) : null}

      {!isOwner ? (
        <ProfileDangerZone
          isDemo={profile.is_demo === true}
          username={profile.username}
          hasPassword={profile.has_password}
          managedAccountId={managedAccountId}
          messageCount={profile.message_count}
        />
      ) : null}
    </div>
  );
}
