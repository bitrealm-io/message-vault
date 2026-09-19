import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import { LockIcon, PersonIcon } from "../../components/icons";
import PasswordField from "../../components/PasswordField";
import TextField from "../../components/TextField";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/auth";
import { useAsyncAction } from "../../lib/useAsyncAction";
import { login as vaultLogin } from "../../lib/vaultApi";

/** Username and password login for a vault running in local auth mode. */
export default function LoginForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const { busy, error, run } = useAsyncAction();

  // A real submit, so the browser treats this as a login: Enter in either
  // field submits without a per-field key handler, and a password manager can
  // offer to fill the credentials and to save them afterwards.
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || disabled) return;
    void run(async () => {
      if (!username.trim()) {
        throw new Error("Username is required.");
      }
      const url = serverUrl.trim();
      // Re-sync the API client with the address this form is showing, the
      // same as `CreateAccountForm` — `connect()` on the login card can
      // leave the client pointed at a bad host while the form still holds
      // the good address.
      setBaseUrl(url);
      const res = await vaultLogin({ username, password });
      // Awaited so the profile lookup inside `login` has decided where to send
      // the user before this form drops its busy state.
      await login(url, res.token, res.account_id);
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

      <PasswordField
        label="Password"
        className="mt-3.5"
        leadingIcon={<LockIcon size={16} />}
        value={password}
        onChange={setPassword}
        name="password"
        autoComplete="current-password"
        showPassword={showPassword}
        onToggle={() => setShowPassword((v) => !v)}
        isDisabled={disabled}
      />

      <AuthSubmitButton disabled={busy || disabled}>
        {busy ? "Logging in…" : "Log in"}
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
