import type { AccountProfile } from "../../lib/account";
import { productVersionOf, productVersionsDiffer } from "../../lib/buildFormat";
import { formatDateTime } from "../../lib/formatDate";
import { useServerInfo } from "../../lib/useServerInfo";
import { sectionTitleClass } from "./profileStyles";

const APP_NAMES = { desktop: "Desktop app", website: "Website" } as const;

/**
 * When an account last logged in and the app it last connected with, for the
 * owner reading an account opened from User Accounts.
 *
 * The app is marked when it comes from a different release than this server;
 * the server serves it all the same, so the mark is for the owner to read, not
 * a fault. Only the Product Version is compared.
 */
export function AccountActivitySection({ profile }: { profile: AccountProfile }) {
  const serverVersion = useServerInfo().data?.version ?? null;
  const appDiffers =
    profile.app_build != null &&
    serverVersion !== null &&
    productVersionsDiffer(profile.app_build, serverVersion);

  return (
    <>
      <h3 className={sectionTitleClass}>Last Login</h3>
      <div className="mb-6 text-[0.875rem] text-text">
        {profile.last_login_at ? formatDateTime(profile.last_login_at) : "Never"}
      </div>

      <h3 className={sectionTitleClass}>App</h3>
      <div className="mb-6 text-[0.875rem] text-text">
        {profile.app && profile.app_build ? (
          <>
            {APP_NAMES[profile.app]}{" "}
            <span className="font-mono text-[0.75rem]">{profile.app_build}</span>
            {appDiffers ? (
              <span className="block text-[0.75rem] text-muted">
                The server is {productVersionOf(serverVersion)}
              </span>
            ) : null}
          </>
        ) : (
          <span className="text-muted">Never connected.</span>
        )}
      </div>
    </>
  );
}
