import type { AccountProfile } from "../../lib/account";
import { productVersionOf, productVersionsDiffer } from "../../lib/buildFormat";
import { formatDateTime } from "../../lib/formatDate";
import { useVaultInfo } from "../../lib/useVaultInfo";
import { sectionTitleClass } from "./profileStyles";

const APP_NAMES = { desktop: "Desktop app", website: "Website" } as const;

/**
 * When an account last logged in and the app it last connected with, for the
 * vault owner reading an account opened from User Accounts.
 *
 * The app is marked when it comes from a different release than this vault;
 * the vault serves it all the same, so the mark is for the owner to read, not
 * a fault. Only the Product Version is compared.
 */
export function AccountActivitySection({ profile }: { profile: AccountProfile }) {
  const vaultVersion = useVaultInfo().data?.version ?? null;
  const appDiffers =
    profile.app_version != null &&
    vaultVersion !== null &&
    productVersionsDiffer(profile.app_version, vaultVersion);

  return (
    <>
      <h3 className={sectionTitleClass}>Last Login</h3>
      <div className="mb-6 text-[0.875rem] text-text">
        {profile.last_login_at ? formatDateTime(profile.last_login_at) : "Never"}
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
    </>
  );
}
