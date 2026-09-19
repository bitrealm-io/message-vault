import { useAccountProfile } from "../../lib/useAccountProfile";
import { ApiTokensSection } from "./ApiTokensSection";
import { ChangePasswordSection } from "./ChangePasswordSection";
import { ProfileDangerZone } from "./ProfileDangerZone";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/** Account settings: username, password, API tokens, danger zone. */
export function AccountSettingsPanel() {
  const { profile, loading, error: loadError } = useAccountProfile();

  if (loadError) {
    return <div className="text-danger">Could not load account: {loadError}</div>;
  }

  if (loading || !profile) {
    return <div className="text-muted">Loading…</div>;
  }

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
      <ChangePasswordSection disabled={isDemo} />

      <ApiTokensSection
        accountCanImport={profile.can_import ?? true}
        accountCanExport={profile.can_export ?? true}
        accountCanDelete={profile.can_delete ?? false}
      />

      <ProfileDangerZone isDemo={isDemo} username={profile.username} />
    </div>
  );
}
