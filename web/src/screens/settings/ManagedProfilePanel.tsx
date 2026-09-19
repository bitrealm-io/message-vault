import { productVersionOf, productVersionsDiffer } from "../../lib/buildFormat";
import { formatDateTime } from "../../lib/formatDate";
import { useSettingsAccount } from "../../lib/useSettingsAccount";
import { useVaultInfo } from "../../lib/useVaultInfo";
import { inputClassName, sectionTitleClass } from "./profileStyles";

const APP_NAMES = { desktop: "Desktop app", website: "Website" } as const;

/**
 * The Profile tab of an account the vault owner opened from User Accounts.
 *
 * The vault keeps an account's name, time zone and handles its own to set, so
 * the owner reads them here and changes none. The address book is the
 * account's contacts, which the owner does not reach.
 *
 * Last login and the app the account last connected with are here too. The
 * app is marked when it comes from a different release than this vault; the
 * vault serves it all the same, so the mark is for the owner to read, not a
 * fault. Only the Product Version is compared.
 */
export function ManagedProfilePanel({ accountId }: { accountId: number }) {
  const { profile, loading, error } = useSettingsAccount(accountId);
  const vaultVersion = useVaultInfo().data?.version ?? null;

  if (error) return <div className="text-danger">Could not load profile: {error}</div>;
  if (loading || !profile) return <div className="text-muted">Loading…</div>;

  const handles = [
    ...profile.phones.map((handle) => ({ handle, service: "phone" })),
    ...profile.emails.map((handle) => ({ handle, service: "email" })),
  ];

  const appDiffers =
    profile.app_version != null &&
    vaultVersion !== null &&
    productVersionsDiffer(profile.app_version, vaultVersion);

  return (
    <div>
      <p className="mb-6 text-[0.813rem] text-muted">
        {profile.username} sets these under their own Settings.
      </p>

      <h3 className={sectionTitleClass}>Display Name</h3>
      <div className="mb-6 max-w-[360px]">
        <input
          type="text"
          aria-label="Display name"
          value={profile.preferred_name ?? ""}
          readOnly
          className={`${inputClassName} !text-muted`}
        />
      </div>

      <h3 className={sectionTitleClass}>Time Zone</h3>
      <div className="mb-6 max-w-[360px]">
        <input
          type="text"
          aria-label="Time zone"
          value={profile.time_zone}
          readOnly
          className={`${inputClassName} !text-muted`}
        />
      </div>

      <h3 className={sectionTitleClass}>Last Login</h3>
      <div className="mb-6 text-[0.875rem] text-text">
        {profile.last_sign_in_at ? formatDateTime(profile.last_sign_in_at) : "Never"}
      </div>

      <h3 className={sectionTitleClass}>App</h3>
      <div className="mb-6 text-[0.875rem] text-text">
        {profile.app && profile.app_version ? (
          <>
            {APP_NAMES[profile.app]}{" "}
            <span className="font-mono text-[0.75rem]">{profile.app_version}</span>
            {appDiffers ? (
              <span className="block text-[0.75rem] text-muted">
                This vault is {productVersionOf(vaultVersion)}
              </span>
            ) : null}
          </>
        ) : (
          <span className="text-muted">Has not connected yet.</span>
        )}
      </div>

      <h3 className={sectionTitleClass}>Handles</h3>
      {handles.length === 0 ? (
        <div className="text-[0.875rem] text-muted">
          No phone or email handles on this account yet.
        </div>
      ) : (
        <div>
          {handles.map((h) => (
            <div
              key={`${h.service}-${h.handle}`}
              className="flex items-center gap-3 border-b border-border py-1.5 text-[0.875rem]"
            >
              <span className="min-w-[7rem] shrink-0 text-muted">{h.service}</span>
              <span className="min-w-0 flex-1">{h.handle}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
