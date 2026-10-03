"use client";

import {
  applyCountryCodeDigitsInput,
  handleCountryCodeKeyDown,
  normalizeCountryCodeDigitsOnBlur,
  toPhoneE164FromParts,
} from "@/lib/phoneE164";
import type { AuthMode } from "@/lib/authMode";
import { formatBuildInfo } from "@/lib/buildInfo";
import { MAX_PASSWORD_LENGTH } from "@/lib/passwordPolicy";
import { useState } from "react";
import { HankoLogin } from "./HankoLogin";
import {
  ChevronDownIcon,
  ChevronRightIcon,
} from "./icons";
import { CheckMark, PasswordField } from "./PasswordField";

type PhoneMode = "usa" | "international";

type Props = {
  authMode?: AuthMode;
  hankoApiUrl?: string;
};

export function LoginScreen({
  authMode = "local",
  hankoApiUrl = "",
}: Props) {
  const buildLabel = formatBuildInfo();
  const [loginUsername, setLoginUsername] = useState("");
  const [loginPassword, setLoginPassword] = useState("");

  const [createOpen, setCreateOpen] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [noPassword, setNoPassword] = useState(false);
  const [preferredName, setPreferredName] = useState("");
  const [phoneMode, setPhoneMode] = useState<PhoneMode>("usa");
  const [countryCode, setCountryCode] = useState("1");
  const [phone, setPhone] = useState("");
  const [submitting, setSubmitting] = useState<"login" | "create" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const effectiveCountryCode = phoneMode === "usa" ? "1" : countryCode;
  const normalizedPhone = toPhoneE164FromParts(effectiveCountryCode, phone);

  const passwordsMatch =
    Boolean(password) &&
    password.length < MAX_PASSWORD_LENGTH &&
    password === passwordConfirm;

  const passwordsOk = noPassword || passwordsMatch;

  const phoneValid = Boolean(normalizedPhone);

  const canCreate =
    Boolean(username.trim()) &&
    Boolean(preferredName.trim()) &&
    Boolean(normalizedPhone) &&
    passwordsOk &&
    submitting == null;

  const login = async () => {
    if (!loginUsername.trim()) return;
    setSubmitting("login");
    setError(null);
    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          username: loginUsername.trim(),
          password: loginPassword,
        }),
      });
      const json = (await res.json()) as { error?: string };
      if (!res.ok) {
        throw new Error(json.error ?? "Invalid user ID or password");
      }
      // Full reload clears any router cache of pre-login /login redirects
      // for protected routes (settings tabs, etc.).
      window.location.assign("/");
      return;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Invalid user ID or password");
    } finally {
      setSubmitting(null);
    }
  };

  const createAccount = async () => {
    if (!canCreate) return;
    setSubmitting("create");
    setError(null);
    try {
      const res = await fetch("/api/auth/accounts", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          username,
          preferredName,
          phone: normalizedPhone ?? phone,
          noPassword,
          password: noPassword ? undefined : password,
        }),
      });
      const json = (await res.json()) as { error?: string };
      if (!res.ok) {
        throw new Error(json.error ?? "Create failed");
      }
      window.location.assign("/");
      return;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Create failed");
    } finally {
      setSubmitting(null);
    }
  };

  if (authMode === "hanko") {
    return (
      <div className="h-full overflow-y-auto">
        <div className="flex min-h-full items-center justify-center p-6">
          <div className="w-full max-w-lg">
            <div className="rounded-xl border border-border bg-elevated p-8 shadow-xl">
              <h1 className="text-center text-2xl font-bold tracking-tight text-text">
                Welcome to the Message Vault
              </h1>
              <div className="mt-8">
                {hankoApiUrl ? (
                  <HankoLogin apiUrl={hankoApiUrl} />
                ) : (
                  <p className="text-center text-[13px] text-danger" role="alert">
                    Hanko is not configured. Set NEXT_PUBLIC_HANKO_API_URL.
                  </p>
                )}
              </div>
            </div>
            <p className="mt-3 text-right font-mono text-[11px] text-muted">
              {buildLabel}
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex min-h-full items-center justify-center p-6">
        <div className="w-full max-w-lg">
          <div className="rounded-xl border border-border bg-elevated p-8 shadow-xl">
          <h1 className="text-center text-2xl font-bold tracking-tight text-text">
            Welcome to the Message Vault
          </h1>

          <section className="mt-8 space-y-4">
            <label className="block">
              <span className="text-[13px] text-text">User ID</span>
              <input
                type="text"
                value={loginUsername}
                onChange={(e) => setLoginUsername(e.target.value)}
                autoComplete="username"
                className="mt-1 w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none focus:border-accent"
              />
            </label>
            <PasswordField
              label="Password"
              value={loginPassword}
              onChange={setLoginPassword}
              autoComplete="current-password"
            />
            <button
              type="button"
              disabled={submitting != null || !loginUsername.trim()}
              onClick={() => void login()}
              className="w-full rounded-md border border-border bg-bg px-4 py-2 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
            >
              {submitting === "login" ? "Logging in…" : "Login"}
            </button>
          </section>

          <div className="mt-8 border-t border-border" />

          <section className="mt-6">
            <button
              type="button"
              aria-expanded={createOpen}
              onClick={() => {
                setCreateOpen((v) => !v);
                setError(null);
              }}
              className="inline-flex items-center gap-1.5 text-left text-[14px] font-medium text-text transition-colors hover:text-accent"
            >
              {createOpen ? (
                <ChevronDownIcon className="size-3.5 shrink-0 opacity-70" />
              ) : (
                <ChevronRightIcon className="size-3.5 shrink-0 opacity-70" />
              )}
              Create a new account
            </button>

            {createOpen && (
              <div className="mt-4 space-y-4">
                <label className="block">
                  <span className="text-[13px] text-text">User ID</span>
                  <input
                    type="text"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    autoComplete="username"
                    className="mt-1 w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none focus:border-accent"
                  />
                </label>
                <PasswordField
                  label="Password"
                  value={password}
                  onChange={setPassword}
                  disabled={noPassword}
                  autoComplete="new-password"
                  showCheck={noPassword || passwordsMatch}
                />
                <PasswordField
                  label="Confirm password"
                  value={passwordConfirm}
                  onChange={setPasswordConfirm}
                  disabled={noPassword}
                  autoComplete="new-password"
                  showCheck={noPassword || passwordsMatch}
                />
                <label className="inline-flex items-center gap-2 text-[13px] text-text">
                  <input
                    type="checkbox"
                    checked={noPassword}
                    onChange={(e) => {
                      const checked = e.target.checked;
                      setNoPassword(checked);
                      if (checked) {
                        setPassword("");
                        setPasswordConfirm("");
                      }
                    }}
                    className="accent-accent"
                  />
                  No password
                </label>

                <label className="block">
                  <span className="text-[13px] text-text">Display name</span>
                  <input
                    type="text"
                    value={preferredName}
                    onChange={(e) => setPreferredName(e.target.value)}
                    placeholder="John Doe"
                    className="mt-1 w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none placeholder:text-muted focus:border-accent"
                  />
                </label>

                <div
                  role="radiogroup"
                  aria-label="Phone number type"
                  className="flex items-center gap-4"
                >
                  <label className="inline-flex items-center gap-2 text-[13px] text-text">
                    <input
                      type="radio"
                      name="phoneMode"
                      checked={phoneMode === "usa"}
                      onChange={() => {
                        setPhoneMode("usa");
                        setCountryCode("1");
                      }}
                      className="accent-accent"
                    />
                    USA
                  </label>
                  <label className="inline-flex items-center gap-2 text-[13px] text-text">
                    <input
                      type="radio"
                      name="phoneMode"
                      checked={phoneMode === "international"}
                      onChange={() => setPhoneMode("international")}
                      className="accent-accent"
                    />
                    International
                  </label>
                </div>

                {phoneMode === "usa" ? (
                  <div className="grid grid-cols-[minmax(12rem,1fr)_16ch] gap-x-2 gap-y-1">
                    <span className="inline-flex items-center text-[13px] text-text">
                      Phone
                      {phoneValid ? <CheckMark /> : null}
                    </span>
                    <span className="text-[13px] text-text">e.164</span>
                    <input
                      type="tel"
                      value={phone}
                      onChange={(e) => setPhone(e.target.value)}
                      placeholder="(555) 555-0100"
                      aria-label="Phone number"
                      className="w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none placeholder:text-muted focus:border-accent"
                    />
                    <div
                      aria-live="polite"
                      className="flex min-h-[2.5rem] min-w-[16ch] shrink-0 items-center rounded-md border border-border bg-bg/60 px-3 py-2 text-[14px] font-mono"
                    >
                      {normalizedPhone ? (
                        <span className="text-text">{normalizedPhone}</span>
                      ) : (
                        <span className="text-muted">—</span>
                      )}
                    </div>
                  </div>
                ) : (
                  <div className="grid grid-cols-[4.25rem_minmax(12rem,1fr)_16ch] gap-x-2 gap-y-1">
                    <span className="text-[13px] text-text">Country</span>
                    <span className="inline-flex items-center text-[13px] text-text">
                      Phone
                      {phoneValid ? <CheckMark /> : null}
                    </span>
                    <span className="text-[13px] text-text">e.164</span>
                    <div className="flex min-h-[2.5rem] items-center overflow-hidden rounded-md border border-border bg-bg">
                      <span
                        aria-hidden
                        className="shrink-0 pl-1.5 text-[14px] font-mono text-muted"
                      >
                        +
                      </span>
                      <input
                        type="tel"
                        value={countryCode}
                        onChange={(e) =>
                          setCountryCode(
                            applyCountryCodeDigitsInput(e.target.value),
                          )
                        }
                        onKeyDown={handleCountryCodeKeyDown}
                        onBlur={(e) =>
                          setCountryCode(
                            normalizeCountryCodeDigitsOnBlur(e.target.value),
                          )
                        }
                        aria-label="Country code"
                        className="min-w-0 flex-1 border-0 bg-transparent py-2 pl-0.5 pr-1 text-left text-[14px] font-mono text-text outline-none"
                      />
                    </div>
                    <input
                      type="tel"
                      value={phone}
                      onChange={(e) => setPhone(e.target.value)}
                      placeholder="(555) 555-0100"
                      aria-label="Phone number"
                      className="w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none placeholder:text-muted focus:border-accent"
                    />
                    <div
                      aria-live="polite"
                      className="flex min-h-[2.5rem] min-w-[16ch] shrink-0 items-center rounded-md border border-border bg-bg/60 px-3 py-2 text-[14px] font-mono"
                    >
                      {normalizedPhone ? (
                        <span className="text-text">{normalizedPhone}</span>
                      ) : (
                        <span className="text-muted">—</span>
                      )}
                    </div>
                  </div>
                )}

                <button
                  type="button"
                  disabled={!canCreate}
                  onClick={() => void createAccount()}
                  className="w-full rounded-md border border-border bg-bg px-4 py-2 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
                >
                  {submitting === "create" ? "Creating…" : "Create"}
                </button>
              </div>
            )}
          </section>

          {error && (
            <p className="mt-4 text-[13px] text-danger" role="alert">
              {error}
            </p>
          )}
          </div>
          <p className="mt-3 text-right font-mono text-[11px] text-muted">
            {buildLabel}
          </p>
        </div>
      </div>
    </div>
  );
}
