"use client";

import { toPhoneE164 } from "@/lib/phoneE164";
import { MAX_PASSWORD_LENGTH } from "@/lib/passwordPolicy";
import { isDeletionUiBlocked } from "@/lib/v1Capabilities";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  ContactPhoneList,
  normalizePhoneRows,
  phonesForSave,
} from "./contactEdit";
import { DeleteAccountDialog } from "./DeleteAccountDialog";
import { ChevronRightIcon } from "./icons";
import { PasswordField } from "./PasswordField";

type AccountData = {
  id: string;
  username: string;
  noPassword: boolean;
  hankoLinked?: boolean;
  hideLocalPassword?: boolean;
  isDemo: boolean;
  preferredName: string | null;
  displayName: string;
  phones: string[];
};

export function SettingsAccountForm() {
  const router = useRouter();
  const [data, setData] = useState<AccountData | null>(null);
  const [username, setUsername] = useState("");
  const [noPassword, setNoPassword] = useState(false);
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [preferredName, setPreferredName] = useState("");
  const [phones, setPhones] = useState<string[]>([""]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deletingMessages, setDeletingMessages] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [dangerZoneOpen, setDangerZoneOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const applyAccount = (json: AccountData) => {
    setData(json);
    setUsername(json.username);
    setNoPassword(json.noPassword);
    setPasswordOpen(false);
    setPassword("");
    setPasswordConfirm("");
    setPreferredName(json.preferredName ?? json.displayName ?? "");
    setPhones(normalizePhoneRows(json.phones));
  };

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account");
      const json = (await res.json()) as AccountData & { error?: string };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t load your account.");
      applyAccount(json);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn’t load your account.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const hideLocalPassword = data?.hideLocalPassword === true;
  const phonesToSave = phonesForSave(phones);
  const savedPhones = data?.phones ?? [];
  const passwordsMatch =
    Boolean(password) &&
    password.length < MAX_PASSWORD_LENGTH &&
    password === passwordConfirm;
  const savedName = data?.preferredName ?? data?.displayName ?? "";
  const dirty =
    data != null &&
    (preferredName !== savedName ||
      (!hideLocalPassword && noPassword !== data.noPassword) ||
      (!hideLocalPassword && password !== "") ||
      (!hideLocalPassword && passwordConfirm !== "") ||
      phonesToSave.length !== savedPhones.length ||
      phonesToSave.some((phone, i) => phone !== savedPhones[i]));
  const canSave =
    dirty &&
    !saving &&
    !deleting;

  const save = async () => {
    if (!canSave) return;
    setError(null);
    setSaved(false);
    if (!preferredName.trim()) {
      setError("Enter your display name.");
      return;
    }
    if (phonesToSave.length === 0) {
      setError("Add at least one phone number.");
      return;
    }
    const invalidPhone = phonesToSave.find((phone) => !toPhoneE164(phone));
    if (invalidPhone) {
      setError(
        `Invalid phone number “${invalidPhone}”. Include the country code, such as +1 555 555 0100.`,
      );
      return;
    }
    const passwordRequired =
      !hideLocalPassword && data?.noPassword === true && !noPassword;
    const changingPassword =
      !hideLocalPassword &&
      (passwordRequired || password !== "" || passwordConfirm !== "");
    if (!hideLocalPassword && !noPassword && changingPassword) {
      if (!password) {
        setError("Enter a new password.");
        return;
      }
      if (password.length >= MAX_PASSWORD_LENGTH) {
        setError("Password must be fewer than 100 characters.");
        return;
      }
      if (password !== passwordConfirm) {
        setError("Passwords do not match.");
        return;
      }
    }
    setSaving(true);
    try {
      const body: {
        preferredName: string;
        phones: string[];
        noPassword?: true;
        password?: string;
      } = {
        preferredName,
        phones: phonesToSave,
      };
      if (!hideLocalPassword) {
        if (data && noPassword !== data.noPassword) {
          if (noPassword) body.noPassword = true;
          else body.password = password;
        } else if (!noPassword && changingPassword) {
          body.password = password;
        }
      }
      const res = await fetch("/api/settings/account", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const json = (await res.json()) as AccountData & { error?: string };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t save your changes.");
      applyAccount(json);
      setSaved(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn’t save your changes.");
    } finally {
      setSaving(false);
    }
  };

  const performDelete = async () => {
    if (isDeletionUiBlocked()) return;
    setDeleting(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account", { method: "DELETE" });
      const json = (await res.json()) as { ok?: boolean; error?: string };
      if (!res.ok || !json.ok) {
        throw new Error(json.error ?? "Couldn’t delete your account.");
      }
      setDeleteDialogOpen(false);
      router.replace("/login");
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn’t delete your account.");
    } finally {
      setDeleting(false);
    }
  };

  const deleteAllMessages = async () => {
    if (isDeletionUiBlocked()) return;
    setDeletingMessages(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/messages", { method: "DELETE" });
      const json = (await res.json()) as { ok?: boolean; error?: string };
      if (!res.ok || !json.ok) {
        throw new Error(json.error ?? "Couldn’t delete your messages.");
      }
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn’t delete your messages.");
    } finally {
      setDeletingMessages(false);
    }
  };

  if (loading) {
    return <p className="text-[14px] text-muted">Loading…</p>;
  }

  return (
    <div className="max-w-xl space-y-10">
      <section>
        <h2 className="text-[12px] font-semibold tracking-wider text-muted uppercase">
          Your login
        </h2>
        <p className="mt-1 text-[13px] text-muted">
          Manage how you sign in to Message Vault.
        </p>

        <div className="mt-4 space-y-4">
          <label className="block">
            <span className="text-[13px] text-text">User ID</span>
            <input
              type="text"
              value={username}
              readOnly
              disabled
              className="mt-1 w-full rounded-md border border-border bg-elevated px-3 py-2 text-[14px] text-text opacity-70 outline-none"
            />
          </label>

          {hideLocalPassword ? (
            <p className="text-[13px] text-muted">
              Sign-in is managed by Hanko. Add or manage a passkey under{" "}
              <Link
                href="/settings/access"
                className="text-accent underline-offset-2 hover:underline"
              >
                Access
              </Link>
              .
            </p>
          ) : (
            <div>
              <div className="flex items-center gap-4">
                <button
                  type="button"
                  aria-expanded={passwordOpen}
                  disabled={noPassword || saving}
                  onClick={() => {
                    setPasswordOpen((open) => {
                      if (open) {
                        setPassword("");
                        setPasswordConfirm("");
                      }
                      return !open;
                    });
                    setError(null);
                  }}
                  className="rounded-md border border-border bg-elevated px-3 py-1.5 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
                >
                  Change password
                </button>
                <label className="inline-flex items-center gap-2 text-[13px] text-text">
                  <input
                    type="checkbox"
                    checked={noPassword}
                    disabled={saving || data?.isDemo}
                    onChange={(event) => {
                      const checked = event.target.checked;
                      setNoPassword(checked);
                      setSaved(false);
                      setError(null);
                      if (checked) {
                        setPasswordOpen(false);
                        setPassword("");
                        setPasswordConfirm("");
                      }
                    }}
                    className="accent-accent disabled:opacity-70"
                  />
                  Sign in without a password
                </label>
              </div>

              {passwordOpen && !noPassword ? (
                <div className="mt-4 space-y-4">
                  <PasswordField
                    label="New password"
                    value={password}
                    onChange={(value) => {
                      setPassword(value);
                      setSaved(false);
                      setError(null);
                    }}
                    autoComplete="new-password"
                    showCheck={passwordsMatch}
                  />
                  <PasswordField
                    label="Confirm new password"
                    value={passwordConfirm}
                    onChange={(value) => {
                      setPasswordConfirm(value);
                      setSaved(false);
                      setError(null);
                    }}
                    autoComplete="new-password"
                    showCheck={passwordsMatch}
                  />
                </div>
              ) : null}
            </div>
          )}

          <div className="space-y-4 border-t border-border pt-6">
            <div>
              <h2 className="text-[12px] font-semibold tracking-wider text-muted uppercase">
                Your identity
              </h2>
              <p className="mt-1 text-[13px] text-muted">
                Used to display your name and recognize your phone numbers in
                messages.
              </p>
            </div>

            <label className="block">
              <span className="text-[13px] text-text">Display name</span>
              <input
                type="text"
                value={preferredName}
                onChange={(e) => {
                  setPreferredName(e.target.value);
                  setSaved(false);
                  setError(null);
                }}
                className="mt-1 w-full rounded-md border border-border bg-elevated px-3 py-2 text-[14px] text-text outline-none focus:border-accent"
              />
            </label>

            <div>
              <span className="text-[13px] text-text">☎ Phone numbers</span>
              <div className="mt-1">
                <ContactPhoneList
                  phones={phones}
                  onChange={(next) => {
                    setPhones(next);
                    setSaved(false);
                    setError(null);
                  }}
                  minFilled={1}
                  placeholder="Phone number"
                  removeLabel="Remove phone number"
                />
              </div>
              <p className="mt-1 text-[12px] text-muted">
                Include the country code. At least one number is required.
              </p>
            </div>
          </div>
        </div>
      </section>

      <div className="flex items-center gap-3">
        <button
          type="button"
          disabled={!canSave}
          onClick={() => void save()}
          className="min-w-[7rem] shrink-0 rounded-md border border-border bg-elevated px-4 py-2 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
        >
          {saving ? "Saving…" : "Save changes"}
        </button>
        <button
          type="button"
          disabled={!dirty || saving}
          onClick={() => {
            if (data) applyAccount(data);
            setError(null);
            setSaved(false);
          }}
          className="shrink-0 rounded-md border border-border bg-transparent px-4 py-2 text-[13px] text-muted transition-colors hover:bg-hover hover:text-text disabled:opacity-50"
        >
          Cancel
        </button>
        {saved && <span className="text-[13px] text-muted">Saved.</span>}
        {error && (
          <span className="text-[13px] text-danger" role="alert">
            {error}
          </span>
        )}
      </div>

      {data && (
        <section className="border-t border-border pt-8">
          <button
            type="button"
            aria-expanded={dangerZoneOpen}
            onClick={() => setDangerZoneOpen((open) => !open)}
            className="flex w-full items-center gap-2 text-left"
          >
            <ChevronRightIcon
              className={`size-4 shrink-0 text-danger/80 transition-transform ${
                dangerZoneOpen ? "rotate-90" : ""
              }`}
            />
            <span className="text-[12px] font-semibold tracking-wider text-danger uppercase">
              Danger zone
            </span>
          </button>
          <p className="mt-1 pl-6 text-[13px] text-muted">
            Delete messages or permanently remove your account.
          </p>

          {dangerZoneOpen && (
            <div className="mt-4 space-y-4 pl-6">
              <div className="flex items-center justify-between gap-4">
                <p className="min-w-0 flex-1 text-[13px] text-muted">
                  Delete all messages and attachments. Your contacts and
                  settings remain.
                </p>
                <button
                  type="button"
                  disabled={
                    isDeletionUiBlocked() ||
                    saving ||
                    deleting ||
                    deletingMessages
                  }
                  onClick={() => void deleteAllMessages()}
                  className="w-40 shrink-0 rounded-md border border-red-500/40 bg-red-500/15 px-4 py-2 text-[13px] text-red-100 transition-colors hover:bg-red-500/25 disabled:opacity-50"
                >
                  {deletingMessages ? "Deleting…" : "Delete all messages"}
                </button>
              </div>

              {!data.isDemo ? (
                <div className="flex items-center justify-between gap-4">
                  <p className="min-w-0 flex-1 text-[13px] text-muted">
                    Delete this account and everything in it.
                  </p>
                  <button
                    type="button"
                    disabled={
                      isDeletionUiBlocked() ||
                      saving ||
                      deleting ||
                      deletingMessages
                    }
                    onClick={() => setDeleteDialogOpen(true)}
                    className="w-40 shrink-0 rounded-md border border-red-500/40 bg-red-500/15 px-4 py-2 text-[13px] text-red-100 transition-colors hover:bg-red-500/25 disabled:opacity-50"
                  >
                    Delete account
                  </button>
                </div>
              ) : null}
            </div>
          )}
        </section>
      )}

      <DeleteAccountDialog
        open={deleteDialogOpen}
        username={data?.username ?? username}
        deleting={deleting}
        onClose={() => {
          if (!deleting) setDeleteDialogOpen(false);
        }}
        onConfirm={() => void performDelete()}
      />
    </div>
  );
}
