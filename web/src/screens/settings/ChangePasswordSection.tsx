import { useState } from "react";
import Button from "../../components/Button";
import { useAuth } from "../../lib/auth";
import { changePassword } from "../../lib/vaultApi";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * Change the signed-in account's own password.
 *
 * Shared by Settings → Account and by the vault owner's console, which has no
 * Settings to reach. Two copies of a password form would be two places for
 * the confirmation rule and the token rotation to drift apart.
 */
export function ChangePasswordSection({ disabled = false }: { disabled?: boolean }) {
  const { updateToken } = useAuth();
  const [currentPw, setCurrentPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [pwMsg, setPwMsg] = useState("");
  const [pwOk, setPwOk] = useState(false);

  const handleChangePassword = async () => {
    setPwMsg("");
    setPwOk(false);
    if (newPw.length < 8) {
      setPwMsg("New password must be at least 8 characters.");
      return;
    }
    if (newPw !== confirmPw) {
      setPwMsg("New password and confirmation do not match.");
      return;
    }
    try {
      const res = await changePassword({
        current_password: currentPw,
        password: newPw,
      });
      // Changing the password rotates the session, so the old token is dead.
      if (res.token) updateToken(res.token);
      setPwOk(true);
      setPwMsg("Password changed.");
      setCurrentPw("");
      setNewPw("");
      setConfirmPw("");
    } catch (e) {
      setPwMsg(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      <h3 className={sectionTitleClass}>Change Password</h3>
      <div className="mb-6 max-w-[360px]">
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
        <Button
          variant="primary"
          onClick={handleChangePassword}
          disabled={disabled || !currentPw || !newPw || !confirmPw}
          size="sm"
        >
          Change password
        </Button>
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
