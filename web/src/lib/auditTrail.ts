/**
 * Words for the Audit Trail: what each entry says happened, and who did it.
 *
 * The server hands each entry out as an action and the counts and names that
 * describe it (`GET /v1/audit-trail`). This module turns one into a line a
 * person reads. It never has more to show than the entry carries: no message
 * text and no conversation, because the trail holds none
 * (`docs/adr/0020-the-audit-trail-outlives-the-account.md`).
 */

import type { components } from "./serverApi.types";

export type AuditEntry = components["schemas"]["AuditEntry"];
type AuditActor = components["schemas"]["AuditActor"];

const APP_NAMES = { desktop: "desktop app", website: "website" } as const;

/** "3 conversations", "1 conversation". */
function count(n: number | null | undefined, one: string, many = `${one}s`): string {
  const value = n ?? 0;
  return `${value.toLocaleString()} ${value === 1 ? one : many}`;
}

/** "import", "import and export", "import, export and delete". */
function permissions(names: string[]): string {
  if (names.length === 1) return names[0];
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** " from the website (0.10.0+343fe0d8)", or nothing when no app was named. */
function fromApp(entry: AuditEntry): string {
  if (!entry.app) return "";
  return ` from the ${APP_NAMES[entry.app]}${entry.app_version ? ` (${entry.app_version})` : ""}`;
}

/** What started a run: the app of a Session, or an API token by label and hint. */
function startedBy(entry: AuditEntry): string {
  if (entry.credential === "api_token") {
    const hint = entry.api_token_hint ? ` (${entry.api_token_hint})` : "";
    return ` with the API token “${entry.api_token_label ?? ""}”${hint}`;
  }
  return fromApp(entry);
}

function importRun(entry: AuditEntry): string {
  const source = entry.source ? ` from ${entry.source}` : "";
  return `Import Run${source}, ${entry.status ?? "running"}: ${count(entry.messages, "message")}, ${count(entry.attachments, "attachment")}${startedBy(entry)}`;
}

function exportRun(entry: AuditEntry): string {
  const scope =
    entry.scope_kind === "query"
      ? `a search of ${entry.scope_list ?? "messages"}`
      : entry.scope_kind === "selection"
        ? "picked conversations and messages"
        : "everything";
  return `Export Run of ${scope}, ${entry.status ?? "running"}: ${count(entry.messages, "message")} in ${count(entry.conversations, "conversation")}${startedBy(entry)}`;
}

function sessionEnded(entry: AuditEntry): string {
  switch (entry.reason) {
    case "logged_out":
      return "Logged out";
    case "replaced":
      return "Session ended by a newer login";
    case "revoked":
      return "Session ended by the server";
    case "expired":
      return "Session expired";
    default:
      return "Session ended";
  }
}

function loginRefused(entry: AuditEntry): string {
  switch (entry.reason) {
    case "unknown_username":
      return `Login refused: no account is named “${entry.username ?? ""}”${fromApp(entry)}`;
    case "wrong_password":
      return `Login refused: wrong password${fromApp(entry)}`;
    case "account_disabled":
      return `Login refused: the account is disabled${fromApp(entry)}`;
    default:
      return `Login refused${fromApp(entry)}`;
  }
}

function permissionsChanged(entry: AuditEntry): string {
  const parts: string[] = [];
  if (entry.permissions_added?.length) {
    parts.push(`allowed ${permissions(entry.permissions_added)}`);
  }
  if (entry.permissions_removed?.length) {
    parts.push(`removed ${permissions(entry.permissions_removed)}`);
  }
  return `Permissions changed: ${parts.join("; ")}`;
}

/** The one line that says what an entry records. */
export function describeAuditEntry(entry: AuditEntry): string {
  switch (entry.action) {
    case "logged_in":
      return `Logged in${fromApp(entry)}`;
    case "session_ended":
      return sessionEnded(entry);
    case "login_refused":
      return loginRefused(entry);
    case "account_created":
      return "Account created";
    case "account_disabled":
      return "Account disabled";
    case "account_enabled":
      return "Account enabled";
    case "password_set":
      return "Password set";
    case "permissions_changed":
      return permissionsChanged(entry);
    case "messages_deleted":
      return `Messages deleted for good: ${count(entry.conversations, "conversation")}, ${count(entry.attachments, "attachment")}`;
    case "conversation_deleted":
      return "A conversation deleted for good from the trash";
    case "trash_emptied":
      return `Trash emptied: ${count(entry.conversations, "conversation")} deleted, ${count(entry.contacts, "contact")} made Unknown`;
    case "account_deleted":
      return "Account deleted";
    case "registration_opened":
      return "Opened Message Crate to new accounts";
    case "registration_closed":
      return "Closed Message Crate to new accounts";
    case "api_token_created":
      return `API token “${entry.api_token_label ?? ""}” made (${entry.api_token_hint ?? ""})`;
    case "api_token_deleted":
      return `API token “${entry.api_token_label ?? ""}” deleted (${entry.api_token_hint ?? ""})`;
    case "address_book_loaded":
      return `Address book loaded (${entry.mode ?? "append"}): ${count(entry.contacts_created, "contact")} added, ${entry.contacts_updated ?? 0} changed, ${entry.contacts_deleted ?? 0} removed`;
    case "address_book_exported":
      return `Address book exported: ${count(entry.contacts, "contact")}, ${count(entry.identities, "Identity", "Identities")}`;
    case "import_run":
      return importRun(entry);
    case "export_run":
      return exportRun(entry);
    default:
      entry.action satisfies never;
      return entry.action;
  }
}

const ACTOR_LABELS: Record<AuditActor, string> = {
  owner: "Owner",
  holder: "Account holder",
  command_line: "Server command",
  server: "Server",
  anonymous: "Login screen",
};

/** Who acted, in the words the table shows. */
export function auditActorLabel(actor: AuditActor): string {
  return ACTOR_LABELS[actor];
}
