import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/auth";
import { useAsyncAction } from "../../lib/useAsyncAction";
import { claimVault } from "../../lib/vaultApi";

/**
 * Create the vault owner, which is the only thing an unclaimed vault offers.
 *
 * The owner manages accounts and holds no messages of their own, so this form
 * asks for a username and a password and nothing else: there is no profile to
 * set up, no time zone to pick, and no vault to arrive in. That is why it
 * finishes with "Create Vault Owner" rather than the "Continue" the account
 * form uses — this step is the whole of it.
 */
export default function ClaimVaultForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const { busy, error, run } = useAsyncAction();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || disabled) return;
    void run(async () => {
      if (!username.trim()) {
        throw new Error("Username is required.");
      }
      // Only the mismatch is checked here. Length is the server's rule, so it
      // stays there rather than being restated and left to drift.
      if (password !== confirmPassword) {
        throw new Error("Passwords do not match.");
      }

      const url = serverUrl.trim();
      setBaseUrl(url);
      const res = await claimVault({ username: username.trim(), password });
      await login(url, res.token, res.account_id);
    });
  };

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
      <p className="mb-5 text-[0.875rem] leading-relaxed text-muted">
        A vault owner is required to create and manage users.
      </p>

      <TextField
        label="Username"
        leadingIcon={<PersonIcon size={16} />}
        value={username}
        onChange={setUsername}
        name="username"
        autoComplete="username"
        isDisabled={disabled}
      />

      <PasswordField
        label="Password"
        className="mt-3.5"
        leadingIcon={<LockIcon size={16} />}
        value={password}
        onChange={setPassword}
        name="new-password"
        autoComplete="new-password"
        showPassword={showPassword}
        onToggle={() => setShowPassword((v) => !v)}
        isDisabled={disabled}
      />

      <PasswordField
        label="Confirm Password"
        className="mt-3.5"
        leadingIcon={<LockIcon size={16} />}
        value={confirmPassword}
        onChange={setConfirmPassword}
        name="confirm-password"
        autoComplete="new-password"
        showPassword={showConfirm}
        onToggle={() => setShowConfirm((v) => !v)}
        isDisabled={disabled}
      />

      <AuthSubmitButton disabled={busy || disabled}>
        {busy ? "Creating…" : "Create Vault Owner"}
      </AuthSubmitButton>

      <div className="mt-auto">
        <AuthErrorFooter error={error} className="h-16" />
      </div>
    </form>
  );
}
