import { useState } from "react";
import Button from "../../components/Button";
import { useAuth } from "../../lib/auth";
import { changePassword, setAccountPassword } from "../../lib/vaultApi";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Change an account's password: the logged-in account's own, or, given
 * `managedAccountId`, one the vault owner has opened from User Accounts.
 *
 * One form for both. Two copies of a password form would be two places for
 * the confirmation rule and the token rotation to drift apart.
 *
 * A user account may have no password, so Settings offers Reset password, which clears it.
 * The vault owner must keep one, so its own Settings pass `canReset={false}`.
 * They also pass `requireCurrent`: the owner's account reaches every other, so
 * the vault asks for the password being replaced before it changes it.
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
  const [currentPw, setCurrentPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [pwMsg, setPwMsg] = useState("");
  const [pwOk, setPwOk] = useState(false);

  /** Store `password` as the new one; an empty string clears it. */
  const savePassword = async (password: string) => {
    setPwMsg("");
    setPwOk(false);
    try {
      if (managedAccountId === undefined) {
        const res = await changePassword(
          requireCurrent ? { password, current_password: currentPw } : { password },
        );
        // Changing the password rotates the session, so the old token is dead.
        if (res.token) updateToken(res.token);
      } else {
        // Someone else's password: the owner's own session is untouched.
        await setAccountPassword(managedAccountId, { password });
      }
      setPwOk(true);
      setPwMsg(
        password ? "Password changed." : "Password reset. This account now has no password.",
      );
      setCurrentPw("");
      setNewPw("");
      setConfirmPw("");
    } catch (e) {
      setPwMsg(e instanceof Error ? e.message : String(e));
    }
  };

  const handleChangePassword = async () => {
    if (newPw !== confirmPw) {
      setPwOk(false);
      setPwMsg("New password and confirmation do not match.");
      return;
    }
    await savePassword(newPw);
  };

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
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={handleChangePassword}
            disabled={disabled || !newPw || !confirmPw || (requireCurrent && !currentPw)}
            size="sm"
          >
            Change password
          </Button>
          {canReset && (
            <Button
              variant="secondary"
              onClick={() => void savePassword("")}
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
