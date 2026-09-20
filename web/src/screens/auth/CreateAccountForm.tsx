import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import Button from "../../components/Button";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";
import { useAsyncAction } from "../../lib/useAsyncAction";
import { createAccount } from "../../lib/vaultApi";
import type { components } from "../../lib/vaultApi.types";

/** What the vault answers when it creates an account. */
export type CreatedAccount = components["schemas"]["CreatedAccountResponse"];

/**
 * New vault account: username plus the password twice.
 *
 * The one form for creating an account, whoever creates it: a stranger on the
 * Login screen of an open vault, and the vault owner under User Accounts. The
 * fields, the checks and the request are the same. What happens next differs,
 * and `onCreated` holds it; so does the wording of the action.
 *
 * This is the first half of creating an account, not the whole of it. The name
 * and phone numbers are not asked for here — the account opens with an empty
 * profile, which sends the user straight to profile setup, and only finishing
 * that leaves them with a fully set up account. The Login screen labels the
 * action "Continue" for that reason.
 */
export default function CreateAccountForm({
  submitLabel,
  busyLabel,
  onBeforeCreate,
  onCreated,
  onCancel,
  disabled = false,
}: {
  submitLabel: string;
  busyLabel: string;
  /** Runs once the fields pass, before the request: the Login screen points the app at the vault here. */
  onBeforeCreate?: () => void;
  /** Runs with the vault's answer. The form stays busy until it settles, and shows what it throws. */
  onCreated: (created: CreatedAccount) => Promise<void> | void;
  /** Given where the form can be put away; adds Cancel beside the action. */
  onCancel?: () => void;
  disabled?: boolean;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const { busy, error, run } = useAsyncAction();

  // A real submit, the same as `LoginForm`: Enter submits from any field, and
  // a password manager can recognise the pair of new-password fields and offer
  // to store what it generates.
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

      onBeforeCreate?.();
      const created = await createAccount({
        username: username.trim(),
        password,
        preferred_name: null,
        phone: null,
      });
      await onCreated(created);
    });
  };

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
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

      {onCancel ? (
        <div className="mt-5 flex gap-2">
          <Button
            variant="secondary"
            disabled={busy || disabled}
            onPress={onCancel}
            className="flex-1"
          >
            Cancel
          </Button>
          <AuthSubmitButton disabled={busy || disabled} className="flex-1">
            {busy ? busyLabel : submitLabel}
          </AuthSubmitButton>
        </div>
      ) : (
        <AuthSubmitButton disabled={busy || disabled}>
          {busy ? busyLabel : submitLabel}
        </AuthSubmitButton>
      )}

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
