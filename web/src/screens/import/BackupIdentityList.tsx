import Button from "../../components/Button";
import {
  type IdentityService,
  identityMessageCounts,
  identityOnProfile,
  identityService,
} from "../../lib/backupIdentity";

const HEAD_CELL =
  "border-b border-border pb-1 pr-4 text-left text-[0.75rem] font-normal text-muted";
const BODY_CELL = "border-b border-border py-1 pr-4 align-middle";

/**
 * The addresses a backup's device sent from, each marked as on the
 * account's profile or not, with an inline add for the ones that are not.
 * Renders as boxed rows on the identity stop, and as a table on the Staging
 * Review, where staging has counted the messages each address sent and
 * received.
 */
export default function BackupIdentityList({
  identities,
  profile,
  onAdd,
  busy,
  error,
  messageCounts,
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
  /**
   * Messages staged under each owner handle. Given, the identities are a
   * table inside a stage of the run, with Sent and Received columns.
   */
  messageCounts?: { handle: string; sent: number; received: number }[];
}) {
  if (identities.length === 0) {
    return (
      <p className="m-0 text-[0.813rem] text-muted">
        This backup doesn't record which account it came from.
      </p>
    );
  }

  if (messageCounts) {
    return (
      <>
        <div className="overflow-x-auto pl-4">
          <table className="w-full border-collapse text-[0.813rem]">
            <thead>
              <tr>
                <th scope="col" className={HEAD_CELL}>
                  Identity
                </th>
                <th scope="col" className={`${HEAD_CELL} text-right`}>
                  Sent
                </th>
                <th scope="col" className={`${HEAD_CELL} text-right`}>
                  Received
                </th>
                <th scope="col" className={HEAD_CELL}>
                  On your profile
                </th>
                <th scope="col" className={`${HEAD_CELL} w-px pr-0`}>
                  <span className="sr-only">Action</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {identities.map((identity) => {
                const matched = profile != null ? identityOnProfile(identity, profile) : null;
                const { sent, received } = identityMessageCounts(identity, messageCounts);
                return (
                  <tr key={identity}>
                    <td className={`${BODY_CELL} text-text [overflow-wrap:anywhere]`}>
                      {identity}
                    </td>
                    <td className={`${BODY_CELL} text-right tabular-nums text-text`}>
                      {sent.toLocaleString()}
                    </td>
                    <td className={`${BODY_CELL} text-right tabular-nums text-text`}>
                      {received.toLocaleString()}
                    </td>
                    <td className={`${BODY_CELL} text-muted`}>
                      {matched == null ? "" : matched ? "Yes" : "No"}
                    </td>
                    <td className={`${BODY_CELL} whitespace-nowrap pr-0 text-right`}>
                      {matched === false ? (
                        <Button
                          variant="ghost"
                          size="chip"
                          onClick={() => void onAdd(identity, identityService(identity))}
                          disabled={busy}
                        >
                          Add to profile
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {error && <p className="m-0 mt-2 text-[0.813rem] text-danger">{error}</p>}
      </>
    );
  }

  return (
    <>
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {identities.map((identity) => {
          const matched = profile != null ? identityOnProfile(identity, profile) : null;
          return (
            <li
              key={identity}
              className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2"
            >
              <span className="text-[0.875rem] text-text">{identity}</span>
              {matched === true && (
                <span className="text-[0.813rem] text-muted">On your profile</span>
              )}
              {matched === false && (
                <span className="flex items-center gap-2">
                  <span className="text-[0.813rem] text-muted">Not on your profile</span>
                  <Button
                    variant="ghost"
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
