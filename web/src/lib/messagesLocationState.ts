import type { Conversation } from "./types";

/** Lightweight contact row carried on location state so the drawer can paint immediately. */
export type OpenContactPreview = {
  id: string;
  name: string;
  addresses?: string[];
  handleCount?: number;
  groups?: string[];
};

export type MessagesLocationState = {
  conversation?: Conversation;
  openContactId?: string;
  openContactPreview?: OpenContactPreview;
};

/** True when the value is a plain object (not null or an array). */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** True when the value looks like a conversation with a positive integer id. */
function isConversation(value: unknown): value is Conversation {
  if (!isRecord(value)) return false;
  return typeof value.id === "number" && Number.isInteger(value.id) && value.id > 0;
}

/** True when every element is a string. */
function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

/** Upper bound for stub rows from location state (real identity lists stay far below this). */
const MAX_OPEN_CONTACT_HANDLE_COUNT = 500;

/**
 * Parse a contact preview from location state.
 * Requires non-empty string `id` and `name`. Optional `addresses`/`groups` must be string arrays;
 * optional `handleCount` must be a non-negative integer at most 500. Returns null on any invalid field.
 */
function asOpenContactPreview(value: unknown): OpenContactPreview | null {
  if (!isRecord(value)) return null;
  if (typeof value.id !== "string" || value.id.length === 0) return null;
  if (typeof value.name !== "string" || value.name.length === 0) return null;

  const out: OpenContactPreview = { id: value.id, name: value.name };

  if ("addresses" in value && value.addresses !== undefined) {
    if (!isStringArray(value.addresses)) return null;
    out.addresses = value.addresses;
  }

  if ("groups" in value && value.groups !== undefined) {
    if (!isStringArray(value.groups)) return null;
    out.groups = value.groups;
  }

  if ("handleCount" in value && value.handleCount !== undefined) {
    if (
      typeof value.handleCount !== "number" ||
      !Number.isInteger(value.handleCount) ||
      value.handleCount < 0 ||
      value.handleCount > MAX_OPEN_CONTACT_HANDLE_COUNT
    ) {
      return null;
    }
    out.handleCount = value.handleCount;
  }

  return out;
}

/** Read conversation and contact-drawer fields from React Router location state. */
export function asMessagesLocationState(state: unknown): MessagesLocationState | null {
  if (!isRecord(state)) return null;

  const out: MessagesLocationState = {};

  if ("conversation" in state && state.conversation !== undefined) {
    if (!isConversation(state.conversation)) return null;
    out.conversation = state.conversation;
  }

  if ("openContactId" in state && state.openContactId !== undefined) {
    if (typeof state.openContactId !== "string") return null;
    out.openContactId = state.openContactId;
  }

  if ("openContactPreview" in state && state.openContactPreview !== undefined) {
    const preview = asOpenContactPreview(state.openContactPreview);
    if (preview && out.openContactId !== undefined && preview.id === out.openContactId) {
      out.openContactPreview = preview;
    }
  }

  if (out.conversation === undefined && out.openContactId === undefined) {
    return null;
  }

  return out;
}
