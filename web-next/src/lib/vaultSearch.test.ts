import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import Database from "better-sqlite3";

import { runWithAccount } from "./accountScope";
import { createAccount, saveAccount } from "./accounts";
import { resetDb } from "./dbCore";
import {
  searchConversationMatches,
  searchVault,
  searchVaultByContact,
  searchVaultContacts,
} from "./search";
import { dbPath } from "./paths";
import { ensureVaultSchema } from "./vaultSchema";

describe("vault search + FTS", () => {
  const prevVaultDb = process.env.VAULT_DB;
  const prevVaultDataDir = process.env.VAULT_DATA_DIR;
  let tmpDir = "";
  let accountId = "";
  let labeledConvId = 0;
  let groupConvId = 0;

  before(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "vault-search-"));
    process.env.VAULT_DB = path.join(tmpDir, "vault.db");
    process.env.VAULT_DATA_DIR = path.join(tmpDir, "data");
    const account = await createAccount({
      username: `search_${Date.now()}`,
      preferredName: "Search User",
      phone: "+15555550100",
    });
    accountId = account.id;
    assert.equal(account.read_only, false);
    const locked = saveAccount(accountId, { read_only: true });
    assert.equal(locked.read_only, true);

    const db = new Database(dbPath());
    try {
      ensureVaultSchema(db);
      const resolveHandle = (raw: string, handleType = "phone"): number => {
        db.prepare(
          `INSERT OR IGNORE INTO handles (account_id, raw, normalized, handle_type, service)
           VALUES (?, ?, ?, ?, 'phone')`,
        ).run(accountId, raw, raw, handleType);
        return Number(
          db
            .prepare(
              `SELECT id FROM handles WHERE account_id = ? AND raw = ?`,
            )
            .pluck()
            .get(accountId, raw),
        );
      };
      const insertConv = db.prepare(
        `INSERT INTO conversations (
           account_id, chat_handle_id, conversation_type,
           group_title, exported_at, source_file
         ) VALUES (?, ?, 'individual', NULL, NULL, 't.json')`,
      );
      const insertMsg = db.prepare(
        `INSERT INTO messages (
           conversation_id, account_id, source, guid, timestamp,
           is_from_me, sort_order, body, subject
         ) VALUES (?, ?, 'imessage', ?, ?, 0, 0, ?, NULL)`,
      );
      const insertContact = db.prepare(
        `INSERT INTO contacts (account_id, preferred_name) VALUES (?, ?)`,
      );
      const insertHandle = db.prepare(
        `INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES (?, ?, ?)`,
      );
      const assignContact = (handle: string, name: string) => {
        const contactId = Number(
          insertContact.run(accountId, name).lastInsertRowid,
        );
        insertHandle.run(accountId, resolveHandle(handle), contactId);
        return contactId;
      };
      const addToLabel = (contactId: number, label: string) => {
        db.prepare(
          `INSERT OR IGNORE INTO contact_labels (account_id, name) VALUES (?, ?)`,
        ).run(accountId, label);
        const labelId = Number(
          (
            db
              .prepare(
                `SELECT id FROM contact_labels WHERE account_id = ? AND name = ?`,
              )
              .get(accountId, label) as { id: number }
          ).id,
        );
        db.prepare(
          `INSERT OR IGNORE INTO contact_label_members (contact_id, label_id)
           VALUES (?, ?)`,
        ).run(contactId, labelId);
      };

      const daysAgo = (days: number) =>
        new Date(Date.now() - days * 86_400_000).toISOString();

      const ftsConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550167")).lastInsertRowid,
      );
      insertMsg.run(
        ftsConvId,
        accountId,
        "g-search-1",
        "2021-06-01T12:00:00Z",
        "unique zebra pineapple vault",
      );

      // Long-known + still active: first ~10y ago, last ~5d ago.
      const activeConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550169")).lastInsertRowid,
      );
      assignContact("+15555550169", "Active");
      insertMsg.run(
        activeConvId,
        accountId,
        "g-active-1",
        daysAgo(3650),
        "hello from long ago",
      );
      insertMsg.run(
        activeConvId,
        accountId,
        "g-active-2",
        daysAgo(5),
        "still chatting recently",
      );

      // Stale + old: first ~10y ago, last ~400d ago.
      const staleConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550170")).lastInsertRowid,
      );
      assignContact("+15555550170", "Stale");
      insertMsg.run(
        staleConvId,
        accountId,
        "g-stale-1",
        daysAgo(3650),
        "old friendship start",
      );
      insertMsg.run(
        staleConvId,
        accountId,
        "g-stale-2",
        daysAgo(400),
        "last contact a while back",
      );

      // Entirely recent: first and last within the last week.
      const recentConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550171")).lastInsertRowid,
      );
      assignContact("+15555550171", "Recent");
      insertMsg.run(
        recentConvId,
        accountId,
        "g-recent-1",
        daysAgo(6),
        "brand new contact",
      );
      insertMsg.run(
        recentConvId,
        accountId,
        "g-recent-2",
        daysAgo(1),
        "just messaged yesterday",
      );

      // Two labeled contacts, one of them inactive, sharing a group chat.
      labeledConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550172")).lastInsertRowid,
      );
      const labeledId = assignContact("+15555550172", "Labeled");
      addToLabel(labeledId, "Family");
      insertMsg.run(
        labeledConvId,
        accountId,
        "g-labeled-1",
        "2022-03-01T12:00:00Z",
        "labeled kumquat note",
      );

      const inactiveConvId = Number(
        insertConv.run(accountId, resolveHandle("+15555550173")).lastInsertRowid,
      );
      const inactiveId = assignContact("+15555550173", "Inactive");
      addToLabel(inactiveId, "Family");
      insertMsg.run(
        inactiveConvId,
        accountId,
        "g-inactive-1",
        "2022-03-02T12:00:00Z",
        "inactive kumquat note",
      );

      groupConvId = Number(
        db
          .prepare(
            `INSERT INTO conversations (
               account_id, chat_handle_id, conversation_type,
               group_title, exported_at, source_file
             ) VALUES (?, ?, 'group', 'Kumquat Crew', NULL, 't.json')`,
          )
          .run(accountId, resolveHandle("chat-kumquat", "other")).lastInsertRowid,
      );
      const insertParticipant = db.prepare(
        `INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (?, ?, ?)`,
      );
      insertParticipant.run(groupConvId, resolveHandle("+15555550172"), "Labeled");
      insertParticipant.run(groupConvId, resolveHandle("+15555550173"), "Inactive");
      insertMsg.run(
        groupConvId,
        accountId,
        "g-group-1",
        "2022-04-01T12:00:00Z",
        "group kumquat plans",
      );
    } finally {
      db.close();
    }
  });

  after(() => {
    if (prevVaultDb === undefined) delete process.env.VAULT_DB;
    else process.env.VAULT_DB = prevVaultDb;
    if (prevVaultDataDir === undefined) delete process.env.VAULT_DATA_DIR;
    else process.env.VAULT_DATA_DIR = prevVaultDataDir;
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("finds messages while the vault is read-only", () => {
    runWithAccount(accountId, () => {
      const result = searchVault("zebra");
      assert.ok(result.totalConversations >= 1);
      assert.ok(result.hits.some((h) => h.topMatch?.snippet.includes("zebra")));
    });
  });

  it("matches OR and prefix* in free text", () => {
    runWithAccount(accountId, () => {
      const either = searchVault("zebra OR pineapple");
      assert.ok(either.totalConversations >= 1);
      const prefix = searchVault("zebr*");
      assert.ok(prefix.totalConversations >= 1);
      assert.ok(
        prefix.hits.some((h) =>
          (h.topMatch?.snippet ?? "").toLowerCase().includes("zebra"),
        ),
      );
    });
  });

  it("scopes message search to contacts matching first:/phone:", () => {
    runWithAccount(accountId, () => {
      const byFirst = searchVault("first:Recent");
      assert.ok(byFirst.totalConversations >= 1);
      assert.ok(
        byFirst.hits.every(
          (hit) =>
            hit.title.includes("Recent") || hit.chatIdentifier.includes("51003"),
        ),
      );
      const byPhone = searchVault("phone:+15555550172");
      assert.ok(byPhone.totalConversations >= 1);
      assert.ok(
        byPhone.hits.some((hit) => hit.chatIdentifier.includes("51004")),
      );
    });
  });

  it("supports from:me, to:, with:, and has:noattachment", () => {
    runWithAccount(accountId, () => {
      const db = new Database(dbPath());
      // Setup inserts all as incoming; flip one to sent for from:me.
      db.prepare(
        `UPDATE messages SET is_from_me = 1
         WHERE account_id = ? AND body = ?`,
      ).run(accountId, "still chatting recently");
      db.close();
      resetDb();

      const fromMe = searchVault("from:me still chatting");
      assert.ok(fromMe.totalConversations >= 1);

      const toMe = searchVault("to:me hello from long");
      assert.ok(toMe.totalConversations >= 1);

      const withHandle = searchVault("with:+15555550169 still");
      assert.ok(withHandle.totalConversations >= 1);

      const noAtt = searchVault("has:noattachment still chatting");
      assert.ok(noAtt.totalConversations >= 1);

      const hasAtt = searchVault("has:attachment still chatting");
      assert.equal(hasAtt.totalConversations, 0);
    });
  });

  it("supports group:none, sort:date-asc, and larger:/smaller:", () => {
    runWithAccount(accountId, () => {
      const db = new Database(dbPath());
      const msg = db
        .prepare(
          `SELECT id FROM messages
           WHERE account_id = ? AND body LIKE '%zebra%'
           LIMIT 1`,
        )
        .get(accountId) as { id: number };
      db.prepare(
        `INSERT INTO attachments (
           message_id, original_name, mime_type, size_bytes
         ) VALUES (?, 'big.pdf', 'application/pdf', ?)`,
      ).run(msg.id, 2 * 1024 * 1024);
      db.close();
      resetDb();

      const flat = searchVault("zebra group:none");
      assert.ok((flat.totalMessages ?? 0) >= 1);
      assert.ok((flat.messageHits?.length ?? 0) >= 1);
      assert.equal(flat.messageHits![0]!.snippet.toLowerCase().includes("zebra"), true);
      assert.ok(flat.messageHits![0]!.attachments.length >= 1);

      const asc = searchVault("still chatting sort:date-asc");
      assert.ok(asc.hits.length >= 1);

      const large = searchVault("larger:1M filetype:document");
      assert.ok(large.totalConversations >= 1);

      const small = searchVault("smaller:1K filetype:document");
      assert.equal(small.totalConversations, 0);
    });
  });

  it("still searches after unlocking", () => {
    saveAccount(accountId, { read_only: false });
    runWithAccount(accountId, () => {
      const result = searchVault("pineapple");
      assert.ok(result.totalConversations >= 1);
    });
  });

  it("lists every match in the given conversations, oldest first", () => {
    runWithAccount(accountId, () => {
      const result = searchConversationMatches("kumquat", [
        labeledConvId,
        groupConvId,
      ]);
      assert.equal(result.matches.length, 2);
      const [first, second] = result.matches;
      assert.ok(first!.timestamp < second!.timestamp);
    });
  });

  it("returns no matches for an empty find query", () => {
    runWithAccount(accountId, () => {
      const result = searchConversationMatches("   ", [labeledConvId]);
      assert.equal(result.matches.length, 0);
    });
  });

  it("searches contacts by name or phone handle", () => {
    runWithAccount(accountId, () => {
      const byName = searchVaultContacts("search:contacts handle:Recent");
      assert.deepEqual(
        byName.contacts?.map((hit) => hit.contact.displayName),
        ["Recent"],
      );
      const byPhone = searchVaultContacts(
        "search:contacts handle:+15555550172",
      );
      assert.deepEqual(
        byPhone.contacts?.map((hit) => hit.contact.displayName),
        ["Labeled"],
      );
    });
  });

  it("searches contacts by first/last/phone and is:nofirst / is:nolast", () => {
    runWithAccount(accountId, () => {
      const byFirst = searchVaultContacts("search:contacts first:Recent");
      assert.deepEqual(
        byFirst.contacts?.map((hit) => hit.contact.displayName),
        ["Recent"],
      );
      const byPhone = searchVaultContacts("search:contacts phone:+15555550172");
      assert.deepEqual(
        byPhone.contacts?.map((hit) => hit.contact.displayName),
        ["Labeled"],
      );

      const db = new Database(dbPath());
      db.prepare(
        `INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES (?, '+15555550175', '+15555550175', 'phone', 'phone'),
                (?, '+15555550174', '+15555550174', 'phone', 'phone')`,
      ).run(accountId, accountId);
      const namelessHandleId = Number(
        db
          .prepare(
            `SELECT id FROM handles WHERE account_id = ? AND raw = '+15555550175'`,
          )
          .pluck()
          .get(accountId),
      );
      const noLastHandleId = Number(
        db
          .prepare(
            `SELECT id FROM handles WHERE account_id = ? AND raw = '+15555550174'`,
          )
          .pluck()
          .get(accountId),
      );
      const namelessId = Number(
        db
          .prepare(
            `INSERT INTO contacts (account_id, preferred_name) VALUES (?, '')`,
          )
          .run(accountId).lastInsertRowid,
      );
      db.prepare(
        `INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES (?, ?, ?)`,
      ).run(accountId, namelessHandleId, namelessId);
      const noLastId = Number(
        db
          .prepare(
            `INSERT INTO contacts (account_id, preferred_name) VALUES (?, ?)`,
          )
          .run(accountId, "OnlyFirst").lastInsertRowid,
      );
      db.prepare(
        `INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES (?, ?, ?)`,
      ).run(accountId, noLastHandleId, noLastId);
      db.close();

      const bothEmpty = searchVaultContacts(
        "search:contacts is:nofirst is:nolast",
      );
      assert.ok(
        bothEmpty.contacts?.some((hit) => hit.contact.id === namelessId),
      );
      assert.ok(
        bothEmpty.contacts?.every(
          (hit) =>
            !(hit.contact.firstName ?? "").trim() &&
            !(hit.contact.lastName ?? "").trim(),
        ),
      );

      const noLast = searchVaultContacts("search:contacts is:nolast");
      assert.ok(noLast.contacts?.some((hit) => hit.contact.id === noLastId));
      assert.ok(
        noLast.contacts?.every((hit) => !(hit.contact.lastName ?? "").trim()),
      );
    });
  });

  it("filters contacts by label and direct/group counts", () => {
    runWithAccount(accountId, () => {
      const grouped = searchVaultContacts(
        'search:contacts within:Family group-count:=1 message-count:=1',
      );
      assert.deepEqual(
        grouped.contacts
          ?.map((hit) => hit.contact.displayName)
          .sort((a, b) => a.localeCompare(b)),
        ["Inactive", "Labeled"],
      );
      const direct = searchVaultContacts(
        "search:contacts group-count:=0 message-count:>1",
      );
      assert.ok(
        direct.contacts?.some((hit) => hit.contact.displayName === "Active"),
      );
      assert.ok(
        direct.contacts?.every(
          (hit) =>
            hit.contact.groupMessageCount === 0 &&
            hit.contact.messageCount > 1,
        ),
      );
    });
  });

  it("filters by last-contact: (last message on or before date)", () => {
    runWithAccount(accountId, () => {
      const cutoff = new Date(Date.now() - 30 * 86_400_000)
        .toISOString()
        .slice(0, 10);
      const result = searchVault(`last-contact:${cutoff}`);
      const handles = result.hits.map((h) => h.chatIdentifier);
      const selectedHit = result.hits.find(
        (h) => h.chatIdentifier === "+15555550170",
      );
      assert.ok(handles.includes("+15555550170"));
      assert.ok(selectedHit?.contactId != null);
      assert.ok(result.contactIds.includes(selectedHit.contactId));
      assert.ok(!handles.includes("+15555550169"));
      assert.ok(!handles.includes("+15555550171"));
    });
  });

  it("filters by first-contact: (first message on or before date)", () => {
    runWithAccount(accountId, () => {
      const cutoff = new Date(Date.now() - 5 * 365 * 86_400_000)
        .toISOString()
        .slice(0, 10);
      const result = searchVault(`first-contact:${cutoff}`);
      const handles = result.hits.map((h) => h.chatIdentifier);
      assert.ok(handles.includes("+15555550169"));
      assert.ok(handles.includes("+15555550170"));
      assert.ok(!handles.includes("+15555550171"));
    });
  });

  it("combines last-contact: and first-contact:", () => {
    runWithAccount(accountId, () => {
      const lastCutoff = new Date(Date.now() - 30 * 86_400_000)
        .toISOString()
        .slice(0, 10);
      const firstCutoff = new Date(Date.now() - 5 * 365 * 86_400_000)
        .toISOString()
        .slice(0, 10);
      const result = searchVault(
        `last-contact:${lastCutoff} first-contact:${firstCutoff}`,
      );
      const handles = result.hits.map((h) => h.chatIdentifier);
      assert.ok(handles.includes("+15555550170"));
      assert.ok(!handles.includes("+15555550169"));
      assert.ok(!handles.includes("+15555550171"));
    });
  });

  it("filters by an on-or-after last-contact bound", () => {
    runWithAccount(accountId, () => {
      const cutoff = new Date(Date.now() - 30 * 86_400_000)
        .toISOString()
        .slice(0, 10);
      const handles = searchVault(`last-contact:>=${cutoff}`).hits.map(
        (h) => h.chatIdentifier,
      );
      assert.ok(handles.includes("+15555550169"));
      assert.ok(handles.includes("+15555550171"));
      assert.ok(!handles.includes("+15555550170"));
    });
  });

  it("filters by a first-contact range", () => {
    runWithAccount(accountId, () => {
      const handles = searchVault(
        "first-contact:2022-02-01..2022-03-02",
      ).hits.map((h) => h.chatIdentifier);
      assert.ok(handles.includes("+15555550172"));
      assert.ok(!handles.includes("+15555550173"));
      assert.ok(!handles.includes("+15555550169"));
    });
  });

  it("within: searches a label's contacts", () => {
    runWithAccount(accountId, () => {
      const handles = searchVault("kumquat within:Family").hits.map(
        (h) => h.chatIdentifier,
      );
      assert.ok(handles.includes("+15555550172"));
      assert.ok(handles.includes("+15555550173"));
      assert.ok(handles.includes("chat-kumquat"));

      const other = searchVault("kumquat within:Nobody").hits;
      assert.equal(other.length, 0);
    });
  });

  it("groups results by contact, nesting shared group chats", () => {
    runWithAccount(accountId, () => {
      const result = searchVaultByContact("kumquat");
      const names = result.contacts?.map((c) => c.contact.displayName) ?? [];
      assert.deepEqual([...names].sort(), ["Inactive", "Labeled"]);
      assert.equal(result.totalContacts, 2);

      for (const hit of result.contacts ?? []) {
        const titles = hit.hits.map((h) => h.title);
        assert.ok(
          titles.includes("Kumquat Crew"),
          `${hit.contact.displayName} should include the shared group chat`,
        );
        assert.equal(hit.hits.length, 2);
        assert.equal(hit.matchCount, 2);
      }
    });
  });

  it("groups group-only matches under every participating contact", () => {
    runWithAccount(accountId, () => {
      const result = searchVaultByContact('"group kumquat plans" is:group');
      const names = (result.contacts ?? []).map((c) => c.contact.displayName);
      assert.deepEqual([...names].sort(), ["Inactive", "Labeled"]);
      for (const hit of result.contacts ?? []) {
        assert.deepEqual(
          hit.hits.map((h) => h.title),
          ["Kumquat Crew"],
        );
      }
    });
  });

  it("only heads contact groups with contacts that passed the filters", () => {
    runWithAccount(accountId, () => {
      // Both share the group chat, but only one is first contacted in March.
      const result = searchVaultByContact(
        "kumquat first-contact:2022-02-28..2022-03-02",
      );
      assert.deepEqual(
        (result.contacts ?? []).map((c) => c.contact.displayName),
        ["Labeled"],
      );
      const titles = result.contacts?.[0]?.hits.map((h) => h.title) ?? [];
      assert.ok(titles.includes("Kumquat Crew"));
    });
  });

  it("returns no contacts when nothing matches", () => {
    runWithAccount(accountId, () => {
      const result = searchVaultByContact("hapaxlegomenonxyz");
      assert.deepEqual(result.contacts, []);
      assert.equal(result.totalContacts, 0);
    });
  });
});
