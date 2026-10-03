"use client";

import {
  applyCountryCodeDigitsInput,
  handleCountryCodeKeyDown,
  normalizeCountryCodeDigitsOnBlur,
  toPhoneE164FromParts,
} from "@/lib/phoneE164";
import { useState } from "react";

type PhoneMode = "usa" | "international";

export function OnboardingForm() {
  const [preferredName, setPreferredName] = useState("");
  const [phoneMode, setPhoneMode] = useState<PhoneMode>("usa");
  const [countryCode, setCountryCode] = useState("1");
  const [phone, setPhone] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const effectiveCountryCode = phoneMode === "usa" ? "1" : countryCode;
  const normalizedPhone = toPhoneE164FromParts(effectiveCountryCode, phone);
  const canSubmit =
    Boolean(preferredName.trim()) &&
    Boolean(normalizedPhone) &&
    !submitting;

  const submit = async () => {
    if (!canSubmit || !normalizedPhone) return;
    setSubmitting(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          preferredName: preferredName.trim(),
          phones: [normalizedPhone],
        }),
      });
      const json = (await res.json()) as { error?: string };
      if (!res.ok) {
        throw new Error(json.error ?? "Couldn’t save your profile.");
      }
      window.location.assign("/");
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Couldn’t save your profile.",
      );
      setSubmitting(false);
    }
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex min-h-full items-center justify-center p-6">
        <div className="w-full max-w-lg">
          <div className="rounded-xl border border-border bg-elevated p-8 shadow-xl">
            <h1 className="text-center text-2xl font-bold tracking-tight text-text">
              Complete your profile
            </h1>
            <p className="mt-2 text-center text-[14px] text-muted">
              Add your name and phone number so we can identify your messages
              when you import them.
            </p>

            <div className="mt-8 space-y-4">
              <label className="block">
                <span className="text-[13px] text-text">Display name</span>
                <input
                  type="text"
                  value={preferredName}
                  onChange={(e) => {
                    setPreferredName(e.target.value);
                    setError(null);
                  }}
                  autoComplete="nickname"
                  placeholder="John Doe"
                  className="mt-1 w-full rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none placeholder:text-muted focus:border-accent"
                />
              </label>

              <div>
                <span className="text-[13px] text-text">Phone number</span>
                <div className="mt-1 flex gap-2">
                  <select
                    value={phoneMode}
                    onChange={(e) =>
                      setPhoneMode(e.target.value as PhoneMode)
                    }
                    className="rounded-md border border-border bg-bg px-2 py-2 text-[13px] text-text outline-none focus:border-accent"
                  >
                    <option value="usa">USA (+1)</option>
                    <option value="international">International</option>
                  </select>
                  {phoneMode === "international" ? (
                    <input
                      type="text"
                      inputMode="numeric"
                      value={countryCode}
                      onChange={(e) =>
                        setCountryCode(
                          applyCountryCodeDigitsInput(e.target.value),
                        )
                      }
                      onKeyDown={handleCountryCodeKeyDown}
                      onBlur={() =>
                        setCountryCode(
                          normalizeCountryCodeDigitsOnBlur(countryCode),
                        )
                      }
                      aria-label="Country code"
                      className="w-20 rounded-md border border-border bg-bg px-2 py-2 text-[14px] text-text outline-none focus:border-accent"
                    />
                  ) : null}
                  <input
                    type="tel"
                    value={phone}
                    onChange={(e) => {
                      setPhone(e.target.value);
                      setError(null);
                    }}
                    autoComplete="tel-national"
                    placeholder="Phone number"
                    className="min-w-0 flex-1 rounded-md border border-border bg-bg px-3 py-2 text-[14px] text-text outline-none focus:border-accent"
                  />
                </div>
                <p className="mt-1 text-[12px] text-muted">
                  Include the country code (saved like +15555550119).
                </p>
              </div>

              <button
                type="button"
                disabled={!canSubmit}
                onClick={() => void submit()}
                className="w-full rounded-md border border-border bg-bg px-4 py-2 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
              >
                {submitting ? "Saving…" : "Continue"}
              </button>

              {error ? (
                <p className="text-center text-[13px] text-danger">{error}</p>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
