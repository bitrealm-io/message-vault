import { describe, expect, it } from "vitest";
import { type AuditEntry, describeAuditEntry } from "./auditTrail";

/** An entry with only who, what and when; each test adds what it describes. */
function entry(fields: Partial<AuditEntry> & Pick<AuditEntry, "action">): AuditEntry {
  return {
    id: 1,
    at: "2026-10-02T10:00:00+00:00",
    actor: "holder",
    account_id: 101,
    username: "alice",
    ...fields,
  };
}

describe("describeAuditEntry", () => {
  it("says how a Session ended", () => {
    expect(describeAuditEntry(entry({ action: "session_ended", reason: "replaced" }))).toBe(
      "Session ended by a newer login",
    );
    expect(describeAuditEntry(entry({ action: "session_ended", reason: "expired" }))).toBe(
      "Session expired",
    );
  });

  it("names the username a refused login typed when no account has it", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "login_refused",
          reason: "unknown_username",
          account_id: null,
          username: "nobody",
          app: "desktop",
          app_version: "0.10.0+bbbb2222",
        }),
      ),
    ).toBe("Login refused: no account is named “nobody” from the desktop app (0.10.0+bbbb2222)");
  });

  it("names the API token that started a run, by label and hint", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "export_run",
          status: "completed",
          scope_kind: "query",
          scope_list: "messages",
          messages: 1,
          conversations: 2,
          credential: "api_token",
          api_token_label: "nightly backup",
          api_token_hint: "mc-api-Sd..mE",
        }),
      ),
    ).toBe(
      "Export Run of a search of messages, completed: 1 message in 2 conversations with the API token “nightly backup” (mc-api-Sd..mE)",
    );
  });

  it("lists the permissions turned on and off", () => {
    expect(
      describeAuditEntry(
        entry({
          action: "permissions_changed",
          actor: "owner",
          permissions_added: ["import"],
          permissions_removed: ["export", "delete"],
        }),
      ),
    ).toBe("Permissions changed: allowed import; removed export and delete");
  });
});
