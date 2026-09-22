/** Messaging service ids used on profiles, setup, and contacts. */
export type HandleService = "phone" | "email" | "whatsapp";

export const HANDLE_SERVICES = [
  "phone",
  "email",
  "whatsapp",
] as const satisfies readonly HandleService[];

/**
 * The services an identity can be on, offered wherever one is added: setup,
 * the account profile, and the contact drawer. Each carries the example shown
 * in an empty field. The example lives next to the service so adding one means
 * adding its example on the same line — there is no second place to forget.
 */
export const HANDLE_SERVICE_OPTIONS = [
  { value: "phone", label: "Text message", placeholder: "+1 555-123-4567" },
  { value: "email", label: "Email", placeholder: "you@example.com" },
  { value: "whatsapp", label: "WhatsApp", placeholder: "+1 555-123-4567" },
] as const satisfies ReadonlyArray<{
  value: HandleService;
  label: string;
  placeholder: string;
}>;

/** Example shown in an empty value field for `service`. */
export function handlePlaceholder(service: HandleService): string {
  return HANDLE_SERVICE_OPTIONS.find((option) => option.value === service)?.placeholder ?? "";
}

/**
 * Why a handle cannot be used, or null when it can. An empty value is not an
 * error here: a blank row is one the person has not filled in yet, and the
 * screens that collect handles decide separately how many they need.
 *
 * The number check counts digits rather than matching a shape, so the
 * separators people actually type — spaces, dots, dashes, parentheses — all
 * pass. Seven digits is the shortest real subscriber number and fifteen is the
 * most E.164 allows.
 */
export function handleValidationError(service: HandleService, value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;

  if (service === "email") {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(trimmed)
      ? null
      : "Enter an email address like you@example.com.";
  }

  const digits = trimmed.replace(/\D/g, "");
  const onlyNumberCharacters = /^\+?[\d\s().-]+$/.test(trimmed);
  if (!onlyNumberCharacters || digits.length < 7 || digits.length > 15) {
    return "Enter a phone number like +1 555-123-4567.";
  }
  return null;
}

/** Shown against the second and later rows carrying an account already listed. */
export const DUPLICATE_HANDLE_MESSAGE = "This account is already in the list.";

/**
 * Key two handles share when they are the same account, or null for a value
 * with nothing to compare. Two rows match when their keys are equal.
 *
 * The service is part of the key because the same number on Text Message and
 * on WhatsApp is two accounts, not one, and listing both is the right thing to
 * do. Within a service the comparison ignores how the value was typed: an
 * email folds to lower case, and a number falls back to its digits so
 * `+1 (555) 123-4567` and `+15551234567` land on the same key.
 *
 * The digits are compared whole rather than by their last ten, so a number
 * written once with its country code and once without is not caught. That is
 * deliberate — guessing at country codes would let this refuse two numbers
 * that really are different, and a missed duplicate costs far less than an
 * error the person cannot talk their way out of.
 */
export function handleDuplicateKey(service: HandleService, value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const normalized =
    service === "email"
      ? trimmed.toLowerCase()
      : trimmed.replace(/\D/g, "") || trimmed.toLowerCase();
  return `${service}:${normalized}`;
}

/**
 * Guess the service for a handle when the stored service is empty.
 * Emails contain `@`. Phone-like values are mostly digits.
 */
export function inferService(handle: string, service: string | null | undefined): string {
  if (service?.trim()) return service.trim().toLowerCase();
  const h = handle.trim();
  if (h.includes("@") && !h.startsWith("@")) return "email";
  if (/^\+?\d[\d\s().-]{6,}$/.test(h)) return "phone";
  return "unknown";
}

/** Label shown in the handles table Service column. */
export function formatHandleServiceLabel(
  handle: string,
  service: string | null | undefined,
): string {
  const lower = inferService(handle, service);
  if (lower === "whatsapp") return "WhatsApp";
  if (
    lower === "phone" ||
    lower === "sms" ||
    lower === "mms" ||
    lower === "sms/mms" ||
    lower === "imessage" ||
    lower === "ios" ||
    lower === "rcs"
  ) {
    return "Text message";
  }
  if (lower === "email") return "Email";
  if (lower === "unknown") return "—";
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}
