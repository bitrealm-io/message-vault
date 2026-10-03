import type { KeyboardEvent } from "react";
import { parsePhoneNumberFromString } from "libphonenumber-js/core";
import metadataJson from "libphonenumber-js/metadata.min.json";

/** Unwrap CJS/ESM interop so metadata always has `.countries`. */
const phoneMetadata =
  (metadataJson as { default?: typeof metadataJson }).default ?? metadataJson;

/** Strip non-digits; drop leading US country code 1 when 11 digits. */
export function sanitizePhoneDigits(input: string): string {
  let digits = input.replace(/\D/g, "");
  if (digits.length === 11 && digits.startsWith("1")) {
    digits = digits.slice(1);
  }
  return digits;
}

export const E164_MAX_DIGITS = 15;
export const US_NATIONAL_DIGITS = 10;
const COUNTRY_CODE_MAX_DIGITS = 3;

/** Characters commonly used when typing phone numbers — auto-removed on input. */
const PHONE_FORMATTING_RE = /[\s().\-./]/g;

/** Remove common phone formatting only; leave other invalid characters for the user to fix. */
export function stripPhoneFormatting(input: string): string {
  return input.replace(PHONE_FORMATTING_RE, "");
}

export function trimPhoneWhitespace(input: string): string {
  return input.trim();
}

export function hasInvalidPhoneCharacters(input: string): boolean {
  const stripped = stripPhoneFormatting(trimPhoneWhitespace(input));
  if (!stripped) return false;
  const digitsOnly = stripped.startsWith("+") ? stripped.slice(1) : stripped;
  return /[^\d]/.test(digitsOnly);
}

export function applyCountryCodeDigitsInput(value: string): string {
  const stripped = stripPhoneFormatting(trimPhoneWhitespace(value));
  return stripped.replace(/\D/g, "").slice(0, COUNTRY_CODE_MAX_DIGITS);
}

/** Restore default US code when the field is left empty. */
export function normalizeCountryCodeDigitsOnBlur(value: string): string {
  const next = applyCountryCodeDigitsInput(value);
  return next === "" ? "1" : next;
}

/** Country code digits (no + prefix). */
export function digitsFromCountryCode(countryCode: string): string {
  return applyCountryCodeDigitsInput(countryCode);
}

/** National number digits, or null when invalid characters remain. */
export function digitsFromNational(nationalNumber: string): string | null {
  const stripped = stripPhoneFormatting(trimPhoneWhitespace(nationalNumber));
  if (!stripped) return "";
  if (/[^\d]/.test(stripped)) return null;
  return stripped;
}

const NAVIGATION_KEYS = new Set([
  "Backspace",
  "Delete",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Tab",
  "Home",
  "End",
]);

function isNavigationKey(key: string): boolean {
  return NAVIGATION_KEYS.has(key);
}

export function handleCountryCodeKeyDown(
  e: KeyboardEvent<HTMLInputElement>,
): void {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (isNavigationKey(e.key)) return;
  if (e.key.length !== 1) return;

  if (!/\d/.test(e.key)) {
    e.preventDefault();
    return;
  }

  const digitCount = digitsFromCountryCode(e.currentTarget.value).length;
  if (digitCount >= COUNTRY_CODE_MAX_DIGITS) {
    e.preventDefault();
  }
}

const E164_RE = /^\+[1-9]\d{1,14}$/;

/** Build E.164 from separate country code and national number fields. */
export function toPhoneE164FromParts(
  countryCode: string,
  nationalNumber: string,
): string | null {
  const codeDigits = digitsFromCountryCode(countryCode);
  if (!codeDigits || !/^[1-9]\d*$/.test(codeDigits)) {
    return null;
  }

  const national = digitsFromNational(nationalNumber);
  if (national === null || !national) return null;

  if (codeDigits === "1") {
    if (national.length !== US_NATIONAL_DIGITS) return null;
    const full = `+1${national}`;
    return E164_RE.test(full) ? full : null;
  }

  const total = codeDigits.length + national.length;
  if (total > E164_MAX_DIGITS || total < 2) return null;

  const full = `+${codeDigits}${national}`;
  return E164_RE.test(full) ? full : null;
}

/**
 * Build E.164 from user input, but only when the value is unambiguous
 * (guarded policy, mirroring the vault's `phone::normalize_certain`):
 * `+`-prefixed values (8–15 digits, country code not starting with 0) and
 * US national numbers (10 digits, or 11 starting with 1). Anything else —
 * trunk-zero nationals like `020 7946 0000`, foreign nationals without `+`,
 * short codes — returns null instead of fabricating a `+0…` value.
 */
export function toPhoneE164(input: string): string | null {
  const trimmed = trimPhoneWhitespace(input);
  if (!trimmed) return null;

  if (hasInvalidPhoneCharacters(trimmed)) return null;

  if (E164_RE.test(trimmed)) {
    const digits = trimmed.slice(1).replace(/\D/g, "");
    if (digits.length > E164_MAX_DIGITS) return null;
    return trimmed;
  }

  if (trimmed.startsWith("+")) {
    // Formatted +-prefixed value: certain when the digit count (country code
    // included) is 8–15 and the country code does not start with 0.
    const digits = stripPhoneFormatting(trimmed).replace(/\D/g, "");
    if (digits.length >= 8 && digits.length <= 15 && !digits.startsWith("0")) {
      return `+${digits}`;
    }
    return null;
  }

  // US national numbers only: 10 digits, or 11 starting with 1
  // (sanitizePhoneDigits strips the leading 1).
  const digits = sanitizePhoneDigits(stripPhoneFormatting(trimmed));
  if (digits.length === 10) return `+1${digits}`;
  return null;
}

/**
 * Format a stored handle for display. Emails and unparseable values pass through.
 * Phones use international spacing from libphonenumber (`+1 941 555 0100`).
 * Storage/matching stays E.164 — call this only at UI boundaries.
 */
export function formatPhoneDisplay(handle: string | null | undefined): string {
  const raw = handle?.trim() ?? "";
  if (!raw) return "";
  if (raw.includes("@")) return raw;
  try {
    const parsed = parsePhoneNumberFromString(raw, phoneMetadata);
    if (!parsed) return raw;
    return parsed.formatInternational();
  } catch {
    return raw;
  }
}

export function parsePhoneE164(input: string): string {
  const e164 = toPhoneE164(input);
  if (e164) return e164;

  throw new Error(
    "Enter a valid phone number with a country code, such as +1 555 555 0100.",
  );
}
