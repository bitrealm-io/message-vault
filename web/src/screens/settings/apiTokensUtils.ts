/** Show a stored API-key hint as `mv-api-xx..yy`, including older starred forms. */
import { formatUnixDate } from "../../lib/formatDate";

export function displayKeyHint(hint: string | null | undefined): string {
  const raw = (hint ?? "").trim();
  if (!raw) return "mv-api-..";
  if (/^(mv-api-|mv-app-).{2}\.\..{2}$/.test(raw)) return raw;
  const stars = raw.match(/^(mv-api-|mv-app-)(.{2}).*\*{2,}(.{2})$/);
  if (stars) return `${stars[1]}${stars[2]}..${stars[3]}`;
  return raw;
}

/** Date a token was created or last used. */
export function formatTokenDate(secs: string | null | undefined): string {
  return formatUnixDate(secs);
}

/** What an API token is allowed to do, as a readable list. */
export function permissionsLabel(token: {
  can_import: boolean;
  can_export: boolean;
  can_delete: boolean;
}): string {
  const parts: string[] = [];
  if (token.can_import) parts.push("Import");
  if (token.can_export) parts.push("Export");
  if (token.can_delete) parts.push("Delete");
  return parts.length > 0 ? parts.join(" / ") : "None";
}

export type ApiTokenItem = {
  id: number;
  label: string;
  can_import: boolean;
  can_export: boolean;
  can_delete: boolean;
  /** Masked secret, e.g. `mv-api-Sd..mE`. */
  token_hint: string;
  created_at: string;
  /** Unix seconds string, or null/absent if never used. */
  last_accessed_at?: string | null;
};

export const thClass = "px-3 py-2 text-left text-[0.75rem] font-bold text-muted";
export const tdClass = "px-3 py-2 text-[0.75rem] text-text align-middle";
export const tdMuted = "px-3 py-2 text-[0.75rem] text-muted align-middle";
