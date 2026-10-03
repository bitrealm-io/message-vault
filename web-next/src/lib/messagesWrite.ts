import fs from "fs";

import { resetDb } from "./dbCore";
import { accountAssetDirs } from "./paths";
import { openWritableVaultDb } from "./vaultSchema";

export type DeletedMessagesResult = {
  conversations: number;
  attachments: number;
};

/**
 * Permanently delete one account's conversations and import staging rows.
 * Contacts, labels, login details, and import token are retained.
 */
export function deleteAllMessagesForAccount(accountId: string): DeletedMessagesResult {
  const assetDirs = accountAssetDirs(accountId);
  const db = openWritableVaultDb();
  try {
    const deleteAll = db.transaction(() => {
      const attachmentRow = db
        .prepare(
          `SELECT COUNT(*) AS count
           FROM attachments a
           JOIN messages m ON m.id = a.message_id
           JOIN conversations c ON c.id = m.conversation_id
           WHERE c.account_id = ?`,
        )
        .get(accountId) as { count: number };
      const conversations = db
        .prepare(`DELETE FROM conversations WHERE account_id = ?`)
        .run(accountId).changes;
      db
        .prepare(`DELETE FROM staging_conversations WHERE account_id = ?`)
        .run(accountId);
      db
        .prepare(`DELETE FROM trashed_conversations WHERE account_id = ?`)
        .run(accountId);
      db.prepare(`DELETE FROM trashed_handles WHERE account_id = ?`).run(accountId);
      return { conversations, attachments: attachmentRow.count };
    });

    const deleted = deleteAll();
    fs.rmSync(assetDirs.assetsDir, { recursive: true, force: true });
    fs.rmSync(assetDirs.assetsConvertedDir, { recursive: true, force: true });
    return deleted;
  } finally {
    db.close();
    resetDb();
  }
}
