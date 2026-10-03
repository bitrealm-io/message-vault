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

  it("uses Copied for convert and compress, whose Staging copies originals, and Skipped for skip", () => {
    for (const mode of ["convert", "compress"] as const) {
      expect(
        formatAttachmentProgress({
          mode,
          done: 1,
          total: 1,
          bytesDone: 0,
          bytesTotal: 0,
        }),
      ).toMatch(/^Copied attachments: 1\/1 /);
    }
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
