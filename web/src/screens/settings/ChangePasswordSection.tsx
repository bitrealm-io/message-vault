import { useState } from "react";
import Button from "../../components/Button";
import { useAuth } from "../../lib/auth";
import { useRouteCache } from "../../lib/routeQuery";
import { changePassword, setAccountPassword } from "../../lib/serverApi";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Change an account's password: the logged-in account's own, or, given
 * `managedAccountId`, one the owner has opened from User Accounts.
 *
 * One form for both. Two copies of a password form would be two places for
 * the confirmation rule and the token rotation to drift apart.
 *
 * A user account may have no password, so Settings offers Reset password, which clears it.
 * The owner must keep one, so its own Settings pass `canReset={false}`.
 * They also pass `requireCurrent`: the owner's account reaches every other, so
 * the server asks for the password being replaced before it changes it.
 *
 * An account's own change, or a reset, also deletes every API Token the
 * account holds (`change_password_on_conn` on the server), so the message
 * says so. The owner setting another account's password leaves that
 * account's tokens alone.
 */
export function ChangePasswordSection({
  disabled = false,
  canReset = true,
  requireCurrent = false,
  managedAccountId,
}: {
  disabled?: boolean;
  canReset?: boolean;
  requireCurrent?: boolean;
  managedAccountId?: number;
}) {
  const { updateToken } = useAuth();
  const cache = useRouteCache();
  const [currentPw, setCurrentPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [pwMsg, setPwMsg] = useState("");
  const [pwOk, setPwOk] = useState(false);

  /**
   * Store `password` as the new one, confirmed by `confirmation`; an empty
   * pair clears it. The server checks the pair, not this screen, so the
   * current password is checked first and the sentences come back in one
   * fixed order.
   */
  const savePassword = async (password: string, confirmation: string) => {
    setPwMsg("");
    setPwOk(false);
    try {
      const body = { password, password_confirmation: confirmation };
      const own = managedAccountId === undefined;
      if (own) {
        const res = await changePassword(
          requireCurrent ? { ...body, current_password: currentPw } : body,
        );
        // Changing the password rotates the session, so the old token is dead.
        if (res.token) updateToken(res.token);
      } else {
        // Someone else's password: the owner's own session is untouched.
        await setAccountPassword(managedAccountId, body);
      }
      // The API Tokens and `has_password` on the profile changed with it.
      cache.invalidateAccount();
      setPwOk(true);
      setPwMsg(passwordMessage(password !== "", own));
      setCurrentPw("");
      setNewPw("");
      setConfirmPw("");
    } catch (e) {
      setPwMsg(e instanceof Error ? e.message : String(e));
    }
  };

  const handleChangePassword = () => savePassword(newPw, confirmPw);

  return (
    <>
      <h3 className={sectionTitleClass}>Change Password</h3>
      <div className="mb-6 max-w-[360px]">
        {requireCurrent && (
          <label className="mb-2 block">
            <span className="mb-1 block text-[0.813rem] font-medium">Current password</span>
            <input
              type="password"
              value={currentPw}
              onChange={(e) => setCurrentPw(e.target.value)}
              autoComplete="current-password"
              disabled={disabled}
              className={inputClassName}
            />
          </label>
        )}
        <label className="mb-2 block">
          <span className="mb-1 block text-[0.813rem] font-medium">New password</span>
          <input
            type="password"
            value={newPw}
            onChange={(e) => setNewPw(e.target.value)}
            autoComplete="new-password"
            disabled={disabled}
            className={inputClassName}
          />
        </label>
        <label className="mb-2 block">
          <span className="mb-1 block text-[0.813rem] font-medium">Confirm new password</span>
          <input
            type="password"
            value={confirmPw}
            onChange={(e) => setConfirmPw(e.target.value)}
            autoComplete="new-password"
            disabled={disabled}
            className={inputClassName}
          />
        </label>
        <div className="mt-4 flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={() => void handleChangePassword()}
            disabled={disabled || !newPw || !confirmPw || (requireCurrent && !currentPw)}
            size="sm"
          >
            Change password
          </Button>
          {canReset && (
            <Button
              variant="secondary"
              onClick={() => void savePassword("", "")}
              disabled={disabled}
              size="sm"
            >
              Reset password
            </Button>
          )}
        </div>
        {pwMsg && (
          <div
            className="mt-1.5 text-[0.813rem]"
            style={{ color: pwOk ? "var(--ok)" : "var(--danger)" }}
          >
            {pwMsg}
          </div>
        )}
      </div>
    </>
  );
}

/** What the form says once the server has stored the new password, or none. */
function passwordMessage(changed: boolean, own: boolean): string {
  if (own) {
    return changed
      ? "Password changed. This account's API Tokens were revoked."
      : "Password reset. This account now has no password, and its API Tokens were revoked.";
  }
  return changed ? "Password changed." : "Password reset. This account now has no password.";
}
