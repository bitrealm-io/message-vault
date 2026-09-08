import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../components/AuthErrorFooter";
import AuthSubmitButton from "../components/AuthSubmitButton";
import { LockIcon } from "../components/icons";
import PasswordField from "../components/PasswordField";
import { useAuth } from "../lib/auth";
import { authCard, authCardBody, authTitle, pageCenter } from "../lib/uiStyles";
import { useFetchAccountProfile } from "../lib/useAccountProfile";
import { useAsyncAction } from "../lib/useAsyncAction";
import { changePassword } from "../lib/vaultApi";

/**
 * Replace the password the vault owner chose for this account.
 *
 * An account the owner creates arrives with a password its holder did not
 * pick, and the vault marks it `must_change_password` until they do. That mark
 * is what makes an owner-created account safe without an invite flow: the
 * owner's choice gets one sign-in and no more.
 *
 * This step comes before profile setup. Owning the password is a smaller and
 * more urgent thing than naming yourself, and until it is done the person
 * signing in is still using a credential someone else knows.
 */
export default function SetPasswordScreen() {
  const { updateToken } = useAuth();
  const refreshProfile = useFetchAccountProfile();
  const [currentPassword, setCurrentPassword] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showCurrent, setShowCurrent] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const { busy, error, run } = useAsyncAction();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    void run(async () => {
      // Only the mismatch is checked here. Length is the server's rule, so it
      // stays there rather than being restated and left to drift.
      if (password !== confirmPassword) {
        throw new Error("Passwords do not match.");
      }

      const res = await changePassword({
        current_password: currentPassword,
        password,
      });
      // Changing the password rotates the session, so the old token is dead
      // the moment this returns.
      updateToken(res.token);
      // Re-read the profile: clearing `must_change_password` is what lets the
      // guard stop sending this account back here.
      await refreshProfile(true);
    });
  };

  return (
    <div className={pageCenter}>
      <div className={authCard}>
        <div className={authCardBody}>
          <h1 className={`${authTitle} mb-2`}>Choose your password</h1>
          <p className="mb-6 text-[0.875rem] leading-relaxed text-muted">
            This account was created with a password the vault owner chose. Pick your own to finish
            signing in.
          </p>

          <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
            <PasswordField
              label="Current Password"
              leadingIcon={<LockIcon size={16} />}
              value={currentPassword}
              onChange={setCurrentPassword}
              name="current-password"
              autoComplete="current-password"
              showPassword={showCurrent}
              onToggle={() => setShowCurrent((v) => !v)}
            />

            <PasswordField
              label="New Password"
              className="mt-3.5"
              leadingIcon={<LockIcon size={16} />}
              value={password}
              onChange={setPassword}
              name="new-password"
              autoComplete="new-password"
              showPassword={showPassword}
              onToggle={() => setShowPassword((v) => !v)}
            />

            <PasswordField
              label="Confirm New Password"
              className="mt-3.5"
              leadingIcon={<LockIcon size={16} />}
              value={confirmPassword}
              onChange={setConfirmPassword}
              name="confirm-password"
              autoComplete="new-password"
              showPassword={showConfirm}
              onToggle={() => setShowConfirm((v) => !v)}
            />

            <AuthSubmitButton disabled={busy}>{busy ? "Saving…" : "Set Password"}</AuthSubmitButton>

            <div className="mt-auto">
              <AuthErrorFooter error={error} className="h-16" />
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
