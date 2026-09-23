import { describe, expect, it } from "vitest";
import { formatAttachmentProgress } from "./attachmentProgressCopy";

describe("formatAttachmentProgress", () => {
  it("says attachments and includes file count and size for copy", () => {
    const line = formatAttachmentProgress({
      mode: "copy",
      done: 120,
      total: 840,
      bytesDone: 1.2 * 1024 * 1024 * 1024,
      bytesTotal: 4 * 1024 * 1024 * 1024,
    });
    expect(line).toContain("attachments");
    expect(line).toContain("120/840");
    expect(line).toMatch(/1\.2 GB/);
    expect(line).toMatch(/4(\.0)? GB/);
    expect(line.startsWith("Copied")).toBe(true);
  });

  it("uses Converted for convert and Skipped for skip", () => {
    expect(
      formatAttachmentProgress({
        mode: "convert",
        done: 1,
        total: 1,
        bytesDone: 0,
        bytesTotal: 0,
      }),
    ).toMatch(/^Converted attachments: 1\/1 /);
    expect(
      formatAttachmentProgress({
        mode: "skip",
        done: 0,
        total: 0,
        bytesDone: 0,
        bytesTotal: 0,
      }),
    ).toBe("Skipped attachments: 0/0 (0 B / 0 B)");
  });
});
