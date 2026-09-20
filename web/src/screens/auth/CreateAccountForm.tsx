import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/auth";
import { useCreateAccountForm } from "./useCreateAccountForm";

/**
 * New vault account: username plus the password twice.
 *
 * The checks and the request are `useCreateAccountForm`'s, which the vault
 * owner's new-account Settings use too; this is how they look on the Login
 * screen, and what follows here is a login.
 *
 * This is the first half of creating an account, not the whole of it. The name
 * and phone numbers are not asked for here — the account opens with an empty
 * profile, which sends the user straight to profile setup, and only finishing
 * that leaves them with a fully set up account. The action is labelled
 * "Continue" for that reason.
 */
export default function CreateAccountForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const {
    username,
    setUsername,
    password,
    setPassword,
    confirmPassword,
    setConfirmPassword,
    busy,
    error,
    submit,
  } = useCreateAccountForm({
    onBeforeCreate: () => setBaseUrl(serverUrl.trim()),
    onCreated: async (created) => {
      // A stranger's registration opens a Session on the new account; the
      // token is absent only when the owner created it, which this form never
      // does.
      if (!created.token) {
        throw new Error("The vault created the account but opened no session.");
      }
      // Awaited so the empty-profile check inside `login` runs before this form
      // drops its busy state, sending the new account on to profile setup.
      await login(serverUrl.trim(), created.token, created.account_id);
    },
  });

  // A real submit, the same as `LoginForm`: Enter submits from any field, and
  // a password manager can recognise the pair of new-password fields and offer
  // to store what it generates.
  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (!disabled) submit();
  };

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={onSubmit}>
      <TextField
        label="Username"
        leadingIcon={<PersonIcon size={16} />}
        value={username}
        onChange={setUsername}
        name="username"
        autoComplete="username"
        isDisabled={disabled}
      />

      {/* The same gap the Login tab puts above its Password field, so the field
          does not shift under the pointer when the tabs are switched. */}
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

      {/* "Continue", not "Create account": this step opens the account but does
          not finish it — the profile setup screen it leads to does. */}
      <AuthSubmitButton disabled={busy || disabled}>
        {busy ? "Continuing…" : "Continue"}
      </AuthSubmitButton>

      {/* Pushed to the foot of the panel so the message lands just above the
          rule that closes the card, clear of the action that produced it. The
          band is taller than the default because the space above it is empty
          anyway, and a message that wraps grows up into it. */}
      <div className="mt-auto">
        <AuthErrorFooter error={error} className="h-16" />
      </div>
    </form>
  );
}
