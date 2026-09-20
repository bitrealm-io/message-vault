import { useState } from "react";
import { useAsyncAction } from "../../lib/useAsyncAction";
import { createAccount } from "../../lib/vaultApi";
import type { components } from "../../lib/vaultApi.types";

/** What the vault answers when it creates an account. */
export type CreatedAccount = components["schemas"]["CreatedAccountResponse"];

/**
 * Creating an account: a username, the password twice, the checks, and the
 * request.
 *
 * Two screens create accounts, and they look nothing alike: Create Account on
 * the Login screen of an open vault, and the Account section of a new
 * account's Settings, which the vault owner opens from User Accounts. Both are
 * this, so the checks and the request are written once. What differs is what
 * follows, which `onCreated` holds: a stranger is logged in to the account,
 * and the owner is taken to its Settings.
 */
export function useCreateAccountForm({
  onBeforeCreate,
  onCreated,
}: {
  /** Runs once the fields pass, before the request: the Login screen points the app at the vault here. */
  onBeforeCreate?: () => void;
  /** Runs with the vault's answer. The form stays busy until it settles, and shows what it throws. */
  onCreated: (created: CreatedAccount) => Promise<void> | void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const { busy, error, run } = useAsyncAction();

  const submit = () => {
    if (busy) return;
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

  return {
    username,
    setUsername,
    password,
    setPassword,
    confirmPassword,
    setConfirmPassword,
    busy,
    error,
    submit,
  };
}
