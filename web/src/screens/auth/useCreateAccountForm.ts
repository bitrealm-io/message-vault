import { useState } from "react";
import { ApiError } from "../../lib/api";
import { createAccount } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { useAsyncAction } from "../../lib/useAsyncAction";

/** What the server answers when it creates an account. */
export type CreatedAccount = components["schemas"]["CreateAccountResponse"];

/**
 * Creating an account: a username, the password twice, the checks, and the
 * request.
 *
 * Two screens create accounts, and they look nothing alike: Create Account on
 * the Login screen of an open Message Crate, and the Account section of a new
 * account's Settings, which the owner opens from User Accounts. Both are
 * this, so the checks and the request are written once. What differs is what
 * follows, which `onCreated` holds: a stranger is logged in to the account,
 * and the owner is taken to its Settings.
 */
export function useCreateAccountForm({
  onBeforeCreate,
  onCreated,
}: {
  /** Runs once the fields pass, before the request: the Login screen points the app at the server here. */
  onBeforeCreate?: () => void;
  /** Runs with the server's answer. The form stays busy until it settles, and shows what it throws. */
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
      let created: CreatedAccount;
      try {
        created = await createAccount({
          username: username.trim(),
          password,
          preferred_name: null,
          phone: null,
        });
      } catch (e: unknown) {
        // A taken username is the server's answer, worded for the log. The
        // form says the one thing the person can act on.
        if (e instanceof ApiError && e.type === "username-taken") {
          throw new Error("Invalid username.");
        }
        throw e;
      }
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
