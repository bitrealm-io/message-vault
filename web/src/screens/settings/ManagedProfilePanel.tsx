import { useSettingsAccount } from "../../lib/useSettingsAccount";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * The Profile tab of an account the vault owner opened from User Accounts.
 *
 * The vault keeps an account's name, time zone and handles its own to set, so
 * the owner reads them here and changes none. The address book is the
 * account's contacts, which the owner does not reach.
 */
export function ManagedProfilePanel({ accountId }: { accountId: number }) {
  const { profile, loading, error } = useSettingsAccount(accountId);

  if (error) return <div className="text-danger">Could not load profile: {error}</div>;
  if (loading || !profile) return <div className="text-muted">Loading…</div>;

  const handles = [
    ...profile.phones.map((handle) => ({ handle, service: "phone" })),
    ...profile.emails.map((handle) => ({ handle, service: "email" })),
  ];

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
