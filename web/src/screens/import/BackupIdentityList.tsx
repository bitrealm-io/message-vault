import Button from "../../components/Button";
import { type IdentityService, identityOnProfile, identityService } from "../../lib/backupIdentity";

/**
 * The addresses a backup's device sent from, each marked as on the
 * account's profile or not, with an inline add for the ones that are not.
 * Renders on the identity stop and as a section on Gate 1.
 */
export default function BackupIdentityList({
  identities,
  profile,
  onAdd,
  busy,
  error,
  rows,
}: {
  identities: string[];
  /** Null while the profile is loading or its fetch failed — marks and
   * add buttons both need it, so both wait on it: each row shows just the
   * identity value, with no mark and no button, until the profile loads. */
  profile: { phones: string[]; emails: string[] } | null;
  onAdd: (value: string, service: IdentityService) => Promise<void>;
  busy?: boolean;
  /** Set after an "Add to profile" call fails or silently didn't add the
   * address — a short factual line shown under the list, not tied to any
   * one row (the failing identity isn't tracked separately). */
  error?: string | null;
  /** Plain rows with no box around each, for use inside a stage of the run. */
  rows?: boolean;
}) {
  if (identities.length === 0) {
    return (
      <p className="m-0 text-[0.813rem] text-muted">
        This backup doesn't record which account it came from.
      </p>
    );
  }

  return (
    <>
      <ul className={`m-0 flex list-none flex-col p-0 ${rows ? "" : "gap-2"}`}>
        {identities.map((identity) => {
          const matched = profile != null ? identityOnProfile(identity, profile) : null;
          return (
            <li
              key={identity}
              className={
                rows
                  ? "flex flex-wrap items-center justify-between gap-x-4 py-0.5 pl-4"
                  : "flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2"
              }
            >
              <span className={`text-text ${rows ? "text-[0.813rem]" : "text-[0.875rem]"}`}>
                {identity}
              </span>
              {matched === true && (
                <span className="text-[0.813rem] text-muted">On your profile</span>
              )}
              {matched === false && (
                <span className="flex items-center gap-2">
                  <span className="text-[0.813rem] text-muted">Not on your profile</span>
                  <Button
                    variant="ghost"
                    size={rows ? "chip" : undefined}
                    onClick={() => void onAdd(identity, identityService(identity))}
                    disabled={busy}
                  >
                    Add to profile
                  </Button>
                </span>
              )}
            </li>
          );
        })}
      </ul>
      {error && <p className="m-0 mt-2 text-[0.813rem] text-danger">{error}</p>}
    </>
  );
}
