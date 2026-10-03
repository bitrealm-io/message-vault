import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, before, describe, it } from "node:test";

import { runWithAccount } from "./accountScope";
import { createAccount, saveAccount } from "./accounts";
import { parseCsvLine } from "./contactsCsv";
import { exportContactsCsvFromDb } from "./contactsCsvExport";
import { createContact } from "./contactsWrite";

describe("exportContactsCsvFromDb", () => {
  const prevVaultDb = process.env.VAULT_DB;
  const prevVaultDataDir = process.env.VAULT_DATA_DIR;
  let tmpDir = "";
  let accountId = "";

  before(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "vault-csv-export-"));
    process.env.VAULT_DB = path.join(tmpDir, "vault.db");
    process.env.VAULT_DATA_DIR = path.join(tmpDir, "data");
    const account = await createAccount({
      username: `csvx_${Date.now()}`,
      preferredName: "Csv Export",
      phone: "+15555550143",
    });
    accountId = account.id;
    saveAccount(accountId, { read_only: false });
  });

  after(() => {
    if (prevVaultDb === undefined) delete process.env.VAULT_DB;
    else process.env.VAULT_DB = prevVaultDb;
    if (prevVaultDataDir === undefined) delete process.env.VAULT_DATA_DIR;
    else process.env.VAULT_DATA_DIR = prevVaultDataDir;
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("exports phones, names, and more than five labels", () => {
    runWithAccount(accountId, () => {
      const labels = ["A", "B", "C", "D", "E", "F"];
      createContact({
        firstName: "Ada",
        lastName: "Lovelace",
        phones: ["+15555550119", "ada@example.com"],
        labels,
      });

      const csv = exportContactsCsvFromDb();
      const lines = csv.trimEnd().split("\n");
      const header = parseCsvLine(lines[0]!);
      assert.ok(header.includes("label_6"));
      assert.equal(header.indexOf("label_6"), 8);

      const row = parseCsvLine(lines[1]!);
      assert.equal(row[0], "+15555550119");
      assert.ok(!row[0]!.includes("@"));
      assert.equal(row[1], "Ada");
      assert.equal(row[2], "Lovelace");
      assert.deepEqual(row.slice(3, 9), labels);
    });
  });
});
