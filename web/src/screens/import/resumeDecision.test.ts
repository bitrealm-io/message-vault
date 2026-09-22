import { describe, expect, it } from "vitest";
import type { ActiveImportSession } from "../../lib/importSession";
import { checkSourceFingerprint, resumeDecisionFor } from "./resumeDecision";

function session(overrides: Partial<ActiveImportSession> = {}): ActiveImportSession {
  return {
    id: 7,
    source: "imessage",
    mode: "append",
    status: "running",
    started_at: "2026-08-30T00:00:00Z",
    stage: "pushing",
    staging_dir: "/home/u/message-vault/staging-260830",
    device_id: "this-device",
    form: { source: "imessage-ios" },
    source_fingerprint: null,
    source_identities: null,
    summary: null,
    ...overrides,
  };
}

describe("resumeDecisionFor", () => {
  it("has nothing to decide without a session", () => {
    expect(
      resumeDecisionFor({
        session: null,
        deviceId: "this-device",
        folder: "missing",
        fingerprint: "unknown",
      }).kind,
    ).toBe("none");
  });

  it("says where a session belongs when another install owns it", () => {
    const decision = resumeDecisionFor({
      session: session({ device_id: "other-device" }),
      deviceId: "this-device",
      folder: "present",
      fingerprint: "unknown",
    });
    expect(decision.kind).toBe("other_device");
  });

  it("offers discard alone when the staging folder is gone", () => {
    const decision = resumeDecisionFor({
      session: session(),
      deviceId: "this-device",
      folder: "missing",
      fingerprint: "unknown",
    });
    expect(decision.kind).toBe("folder_missing");
  });

  it("says the folder could not be checked when the stat itself failed", () => {
    // An IPC error is not evidence the folder is gone: the panel must not
    // offer to discard staged work on the strength of one.
    for (const stage of ["pushing", "awaiting_gate_1", "transcode", "write", "parse"] as const) {
      expect(
        resumeDecisionFor({
          session: session({ stage }),
          deviceId: "this-device",
          folder: "unknown",
          fingerprint: "unknown",
        }).kind,
      ).toBe("folder_unknown");
    }
  });

  it("puts a session that never recorded a folder ahead of the folder check", () => {
    expect(
      resumeDecisionFor({
        session: session({ staging_dir: null }),
        deviceId: "this-device",
        folder: "unknown",
        fingerprint: "unknown",
      }).kind,
    ).toBe("folder_missing");
  });

  it("resumes the upload when a push was interrupted", () => {
    const decision = resumeDecisionFor({
      session: session({ stage: "pushing" }),
      deviceId: "this-device",
      folder: "present",
      fingerprint: "unknown",
    });
    expect(decision.kind).toBe("resume_push");
  });

  it("restarts when the run died before it had written anything", () => {
    // `write` used to land here too; it resumes now, since the conversations
    // already copied are work worth keeping.
    expect(
      resumeDecisionFor({
        session: session({ stage: "parse" }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "match",
      }).kind,
    ).toBe("restart");
  });

  it("sends a session waiting at a gate back to its gate", () => {
    for (const stage of ["awaiting_gate_1", "awaiting_gate_2"] as const) {
      const decision = resumeDecisionFor({
        session: session({ stage }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "unknown",
      });
      expect(decision.kind).toBe("resume_gate");
    }
  });

  it("sends a session that died converting back to the media pass", () => {
    const decision = resumeDecisionFor({
      session: session({ stage: "transcode" }),
      deviceId: "this-device",
      folder: "present",
      fingerprint: "unknown",
    });
    expect(decision.kind).toBe("resume_media");
  });

  it("still offers discard only when the folder is gone at a gate", () => {
    // Decision 36: after a review, discard only. There is nothing to
    // recompute a summary from.
    for (const stage of ["awaiting_gate_1", "awaiting_gate_2", "transcode"] as const) {
      expect(
        resumeDecisionFor({
          session: session({ stage }),
          deviceId: "this-device",
          folder: "missing",
          fingerprint: "unknown",
        }).kind,
      ).toBe("folder_missing");
    }
  });

  it("treats a missing device id as this install rather than locking the user out", () => {
    expect(
      resumeDecisionFor({
        session: session({ device_id: null }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "unknown",
      }).kind,
    ).toBe("resume_push");
  });
  it("offers to pick up a copy that was interrupted", () => {
    const decision = resumeDecisionFor({
      session: session({ stage: "write" }),
      deviceId: "this-device",
      folder: "present",
      fingerprint: "match",
    });
    expect(decision.kind).toBe("resume_write");
  });

  it("still offers to pick up when the backup cannot be checked", () => {
    expect(
      resumeDecisionFor({
        session: session({ stage: "write" }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "unknown",
      }).kind,
    ).toBe("resume_write");
  });

  it("says the backup changed rather than copying against a different source", () => {
    for (const fingerprint of ["mismatch", "source_missing"] as const) {
      const decision = resumeDecisionFor({
        session: session({ stage: "write" }),
        deviceId: "this-device",
        folder: "present",
        fingerprint,
      });
      expect(decision.kind).toBe("source_changed");
    }
  });

  it("restarts a session that died in parse, whatever the backup looks like", () => {
    expect(
      resumeDecisionFor({
        session: session({ stage: "parse" }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "mismatch",
      }).kind,
    ).toBe("restart");
  });

  it("ignores the fingerprint once the copy is done", () => {
    // Decision 36: a changed source is irrelevant at either gate and during
    // the push — the staged folder is what those stages work from.
    expect(
      resumeDecisionFor({
        session: session({ stage: "pushing" }),
        deviceId: "this-device",
        folder: "present",
        fingerprint: "mismatch",
      }).kind,
    ).toBe("resume_push");
  });

  it("puts a missing folder ahead of any fingerprint answer", () => {
    expect(
      resumeDecisionFor({
        session: session({ stage: "write" }),
        deviceId: "this-device",
        folder: "missing",
        fingerprint: "match",
      }).kind,
    ).toBe("folder_missing");
  });
});

describe("checkSourceFingerprint", () => {
  const stored = {
    path: "/backups/chat.db",
    size_bytes: 1000,
    modified_unix_ms: 1_700_000_000_000,
    message_count: null,
  };

  it("cannot judge a session that stored no fingerprint", () => {
    expect(
      checkSourceFingerprint(null, {
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 1000,
        modifiedUnixMs: 1_700_000_000_000,
      }),
    ).toBe("unknown");
  });

  it("reports a source it could not find", () => {
    expect(checkSourceFingerprint(stored, null)).toBe("source_missing");
    expect(
      checkSourceFingerprint(stored, {
        exists: false,
        isFile: false,
        isDirectory: false,
        sizeBytes: 0,
        modifiedUnixMs: null,
      }),
    ).toBe("source_missing");
  });

  it("matches when size and modified time both agree", () => {
    expect(
      checkSourceFingerprint(stored, {
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 1000,
        modifiedUnixMs: 1_700_000_000_000,
      }),
    ).toBe("match");
  });

  it("notices a different size or a different modified time", () => {
    expect(
      checkSourceFingerprint(stored, {
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 2000,
        modifiedUnixMs: 1_700_000_000_000,
      }),
    ).toBe("mismatch");
    expect(
      checkSourceFingerprint(stored, {
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 1000,
        modifiedUnixMs: 1_700_000_000_001,
      }),
    ).toBe("mismatch");
  });
});
