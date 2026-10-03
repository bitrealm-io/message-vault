import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  contactsCsvHeader,
  parseCsvLine,
  serializeContactsCsv,
} from "./contactsCsv";

describe("serializeContactsCsv", () => {
  it("writes at least five label columns", () => {
    const csv = serializeContactsCsv([
      {
        phones: ["+15555550119"],
        preferredName: "Ada Lovelace",
        labels: ["Family"],
      },
    ]);
    const header = parseCsvLine(csv.split("\n")[0]!);
    assert.deepEqual(header, contactsCsvHeader(5));
    assert.ok(header.includes("label_5"));
  });

  it("expands beyond five labels", () => {
    const labels = ["A", "B", "C", "D", "E", "F", "G"];
    const csv = serializeContactsCsv([
      {
        phones: ["+15555550119"],
        preferredName: "Mononym",
        labels,
      },
    ]);
    const lines = csv.trimEnd().split("\n");
    const header = parseCsvLine(lines[0]!);
    assert.deepEqual(header, contactsCsvHeader(7));
    const row = parseCsvLine(lines[1]!);
    assert.equal(row[0], "+15555550119");
    assert.equal(row[1], "Mononym");
    assert.equal(row[2], "");
    assert.deepEqual(row.slice(3), labels);
  });
});
