import type { FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import { keys } from "../../lib/vaultKeys";
import { useVaultCache } from "../../lib/vaultQuery";
import { useCreateAccountForm } from "../auth/useCreateAccountForm";
import { inputClassName, sectionTitleClass } from "./profileStyles";

/**
 * The Account section of an account that does not exist yet, which the vault
 * owner opens with Add account under User Accounts.
 *
 * It is laid out as `AccountSettingsPanel` is, with the two differences that
 * make it a new account's: the username can be typed, and the action is
 * Create. Status, permissions and the deletions are an existing account's, so
 * they are not here. Create opens the new account's Settings, where they are.
 *
 * The checks and the request are `useCreateAccountForm`'s, the same ones
 * Create Account on the Login screen runs. The vault opens no session for an
 * account its owner creates, so the owner stays logged in.
 */
export function NewAccountPanel() {
  const navigate = useNavigate();
  const cache = useVaultCache();
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
    onCreated: async (created) => {
      // The vault answers with the whole account row, so the account's
      // Settings draw from it at once instead of showing a loading state and
      // redrawing when the fetch lands. The session token is not the row's.
      const { token: _token, ...account } = created;
      cache.set(keys.ownerAccounts.member(account.account_id), account);
      await cache.invalidate(keys.ownerAccounts.all);
      // `replace`, so Back from the account's Settings is User Accounts, not this form.
      navigate(`/owner/accounts/${created.account_id}`, { replace: true });
    },
  });

  // A real submit, so Enter creates the account from any field.
  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    submit();
  };

  return (
    <form onSubmit={onSubmit}>
      <h3 className={sectionTitleClass}>Username</h3>
      <div className="mb-6 max-w-[360px]">
        <input
          type="text"
          aria-label="Username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          autoComplete="off"
          disabled={busy}
          className={inputClassName}
        />
      </div>

      <h3 className={sectionTitleClass}>Password (optional)</h3>
      <div className="mb-6 max-w-[360px]">
        <label className="mb-2 block">
          <span className="mb-1 block text-[0.813rem] font-medium">Password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="new-password"
            disabled={busy}
            className={inputClassName}
          />
        </label>
        <label className="mb-2 block">
          <span className="mb-1 block text-[0.813rem] font-medium">Confirm password</span>
          <input
            type="password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
            autoComplete="new-password"
            disabled={busy}
            className={inputClassName}
          />
        </label>
        <Button variant="primary" type="submit" disabled={busy} size="sm">
          {busy ? "Creating…" : "Create"}
        </Button>
        {error && (
          <div className="mt-1.5 text-[0.813rem] text-danger" role="alert">
            {error}
          </div>
        )}
      </div>
    </form>
  );
}
