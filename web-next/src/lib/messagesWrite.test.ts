import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import Database from "better-sqlite3";

import { createAccount, getAccount } from "./accounts";
import { deleteAllMessagesForAccount } from "./messagesWrite";
import { accountDataDir, dbPath } from "./paths";
import { ensureVaultSchema } from "./vaultSchema";

describe("deleteAllMessagesForAccount", () => {
  const previousDb = process.env.VAULT_DB;
  const previousDataDir = process.env.VAULT_DATA_DIR;
  let tempDir = "";
  let accountId = "";

  before(async () => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "vault-delete-messages-"));
    process.env.VAULT_DB = path.join(tempDir, "vault.db");
    process.env.VAULT_DATA_DIR = path.join(tempDir, "data");
    accountId = (
      await createAccount({
        username: `delete_messages_${Date.now()}`,
        preferredName: "Delete Messages",
        phone: "+15555550123",
      })
    ).id;
  });

  after(() => {
    if (previousDb === undefined) delete process.env.VAULT_DB;
    else process.env.VAULT_DB = previousDb;
    if (previousDataDir === undefined) delete process.env.VAULT_DATA_DIR;
    else process.env.VAULT_DATA_DIR = previousDataDir;
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  it("deletes attachment rows and files while the account is locked", () => {
    const db = new Database(dbPath());
    try {
      ensureVaultSchema(db);
      db.prepare(
        `INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES (?, '+15555550167', '+15555550167', 'phone', 'phone')`,
      ).run(accountId);
      const handleId = Number(
        db
          .prepare(
            `SELECT id FROM handles WHERE account_id = ? AND raw = '+15555550167'`,
          )
          .pluck()
          .get(accountId),
      );
      const conversationId = Number(
        db
          .prepare(
            `INSERT INTO conversations (
               account_id, chat_handle_id, conversation_type,
               group_title, exported_at, source_file
             ) VALUES (?, ?, 'individual', NULL, NULL, 'test.json')`,
          )
          .run(accountId, handleId).lastInsertRowid,
      );
      const messageId = Number(
        db
          .prepare(
            `INSERT INTO messages (
               conversation_id, account_id, source, guid, timestamp,
               is_from_me, sort_order, body, subject
             ) VALUES (?, ?, 'imessage', 'delete-attachment-guid',
                       '2025-01-01T00:00:00Z', 0, 0, 'photo', NULL)`,
          )
          .run(conversationId, accountId).lastInsertRowid,
      );
      db.prepare(
        `INSERT INTO attachments (
           message_id, mime_type, original_name, assets_path,
           derived_mime_type, derived_assets_path
         ) VALUES (?, 'image/jpeg', 'photo.jpg', 'photo.jpg',
                   'image/webp', 'photo.webp')`,
      ).run(messageId);
    } finally {
      db.close();
    }

    const accountRoot = accountDataDir(accountId);
    const assetsDir = path.join(accountRoot, "assets");
    const convertedDir = path.join(accountRoot, "assets_converted");
    fs.mkdirSync(assetsDir, { recursive: true });
    fs.mkdirSync(convertedDir, { recursive: true });
    fs.writeFileSync(path.join(assetsDir, "photo.jpg"), "original");
    fs.writeFileSync(path.join(convertedDir, "photo.webp"), "converted");

    const deleted = deleteAllMessagesForAccount(accountId);
    assert.equal(deleted.conversations, 1);
    assert.equal(deleted.attachments, 1);
    assert.equal(fs.existsSync(assetsDir), false);
    assert.equal(fs.existsSync(convertedDir), false);
    assert.ok(getAccount(accountId));

    const verifyDb = new Database(dbPath(), { readonly: true });
    try {
      assert.equal(
        (
          verifyDb.prepare(`SELECT COUNT(*) AS n FROM messages`).get() as {
            n: number;
          }
        ).n,
        0,
      );
      assert.equal(
        (
          verifyDb.prepare(`SELECT COUNT(*) AS n FROM attachments`).get() as {
            n: number;
          }
        ).n,
        0,
      );
    } finally {
      verifyDb.close();
    }
  });
});
