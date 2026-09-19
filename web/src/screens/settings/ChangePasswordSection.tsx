import { useState } from "react";
import Button from "../../components/Button";
import { useAuth } from "../../lib/auth";
import { changePassword } from "../../lib/vaultApi";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Change the signed-in account's own password.
 *
 * Shared by Settings → Account and by Owner Home, which has no Settings to
 * reach. Two copies of a password form would be two places for
 * the confirmation rule and the token rotation to drift apart.
 *
 * A user account may have no password, so Settings offers Reset password, which clears it.
 * The vault owner must keep one, so Owner Home passes `canReset={false}`.
 */
export function ChangePasswordSection({
  disabled = false,
  canReset = true,
}: {
  disabled?: boolean;
  canReset?: boolean;
}) {
  const { updateToken } = useAuth();
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [pwMsg, setPwMsg] = useState("");
  const [pwOk, setPwOk] = useState(false);

  /** Store `password` as the new one; an empty string clears it. */
  const savePassword = async (password: string) => {
    setPwMsg("");
    setPwOk(false);
    try {
      const res = await changePassword({
        password,
      });
      // Changing the password rotates the session, so the old token is dead.
      if (res.token) updateToken(res.token);
      setPwOk(true);
      setPwMsg(
        password ? "Password changed." : "Password reset. This account now has no password.",
      );
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
            disabled={disabled || !newPw || !confirmPw}
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
