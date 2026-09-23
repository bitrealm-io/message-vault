import { phonesMatch } from "./phoneTokens";

/** Which kind of address a backup identity is, for display and for the profile endpoint. */
export type IdentityService = "phone" | "email";

/** Anything with an `@` is an email; everything else is a phone. */
export function identityService(value: string): IdentityService {
  return value.includes("@") ? "email" : "phone";
}

/** Whether one backup identity is on the account's profile. */
export function identityOnProfile(
  value: string,
  profile: { phones: string[]; emails: string[] },
): boolean {
  if (identityService(value) === "email") {
    const needle = value.trim().toLowerCase();
    return profile.emails.some((email) => email.trim().toLowerCase() === needle);
  }
  return profile.phones.some((phone) => phonesMatch(value, phone));
}

/**
 * Messages staged under one backup identity, sent and received. A handle is
 * the same address as the identity when it matches the way a profile entry
 * would, so two spellings of one phone number count together.
 */
export function identityMessageCounts(
  identity: string,
  ownerHandles: { handle: string; sent: number; received: number }[],
): { sent: number; received: number } {
  const address =
    identityService(identity) === "email"
      ? { phones: [], emails: [identity] }
      : { phones: [identity], emails: [] };
  return ownerHandles
    .filter(({ handle }) => identityOnProfile(handle, address))
    .reduce(
      (total, { sent, received }) => ({
        sent: total.sent + sent,
        received: total.received + received,
      }),
      { sent: 0, received: 0 },
    );
}

/**
 * Whether Import should stop before creating the session: identities were
 * read and none is on the profile. Fails open — no identities read, or no
 * profile loaded (fetch failed), never blocks an import.
 */
export function needsIdentityStop(
  identities: string[],
  profile: { phones: string[]; emails: string[] } | null,
): boolean {
  if (identities.length === 0 || profile === null) return false;
  return !identities.some((identity) => identityOnProfile(identity, profile));
}

/** The session's stored identity list, or null when absent or malformed. */
export function parseSourceIdentities(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  return value.every((item) => typeof item === "string") ? (value as string[]) : null;
}
