import { describe, expect, it } from "vitest";
import type { ImportProgressEvent } from "../../lib/types";
import {
  attachmentDoneDetail,
  EMPTY_TIMING,
  isProgressStepComplete,
  recordStageTime,
  type StageTiming,
  setupDetail,
  stageDurations,
  stepIndexFor,
  stepsFor,
} from "./importProgressState";

describe("isProgressStepComplete", () => {
  it("does not complete attachments when the last clone is reported", () => {
    expect(isProgressStepComplete("attachments", 5, 5)).toBe(false);
    expect(isProgressStepComplete("attachments", 0, 0)).toBe(false);
  });

  it("completes prepare and upload when done reaches total", () => {
    expect(isProgressStepComplete("prepare", 1, 1)).toBe(true);
    expect(isProgressStepComplete("upload", 2, 2)).toBe(true);
    expect(isProgressStepComplete("upload", 1, 2)).toBe(false);
  });

  it("never completes Staging on a setup or parse step, even the last one", () => {
    // "5/5" here means the fifth decrypt step, not the last message; the
    // messages have not been read yet when it arrives. And the last message
    // read still leaves the attachments and the conversation files to
    // stage on the same row.
    expect(isProgressStepComplete("setup", 5, 5)).toBe(false);
    expect(isProgressStepComplete("parse", 10, 10)).toBe(false);
  });
});

describe("setupDetail", () => {
  it("shows the step label with its position", () => {
    expect(setupDetail({ done: 1, total: 5, status: "Deriving backup keys" })).toBe(
      "Deriving backup keys (1/5)",
    );
  });

  it("falls back to a plain label when the event carries none", () => {
    expect(setupDetail({ done: 0, total: 0 })).toBe("Preparing");
  });
});

describe("attachmentDoneDetail", () => {
  it("uses zero counts when no attachments event arrived", () => {
    expect(attachmentDoneDetail("skip", null)).toBe("Skipped attachments: 0/0 (0 B / 0 B)");
    expect(attachmentDoneDetail("copy", null)).toBe("Copied attachments: 0/0 (0 B / 0 B)");
  });

  it("formats the last live counts when present", () => {
    expect(attachmentDoneDetail("copy", { done: 2, total: 4, bytesDone: 10, bytesTotal: 20 })).toBe(
      "Copied attachments: 2/4 (10 B / 20 B)",
    );
  });
});

describe("stepsFor", () => {
  it("shows the three stages under convert and compress", () => {
    expect(stepsFor("convert").map((s) => s.label)).toEqual(["Staging", "Media", "Upload"]);
    expect(stepsFor("compress").map((s) => s.label)).toEqual(["Staging", "Media", "Upload"]);
  });

  it("has no Media stage under copy or skip", () => {
    // There is no Media stage in these modes, so a greyed-out row would be
    // promising work that will never run.
    expect(stepsFor("copy").map((s) => s.label)).toEqual(["Staging", "Upload"]);
    expect(stepsFor("skip").map((s) => s.label)).toEqual(["Staging", "Upload"]);
  });

  it("never says transcode, gate, or step", () => {
    for (const mode of ["copy", "convert", "compress", "skip"] as const) {
      for (const step of stepsFor(mode)) {
        for (const avoided of ["transcode", "gate", "step"]) {
          expect(step.label.toLowerCase()).not.toContain(avoided);
        }
      }
    }
  });
});

describe("stepIndexFor", () => {
  it("puts reading, copying and writing all on the Staging row", () => {
    // Decrypting, parsing, copying attachments and writing the conversation
    // files ("prepare") are all Staging from the person's side.
    for (const step of ["setup", "parse", "attachments", "prepare"] as const) {
      expect(stepIndexFor(step, "convert")).toBe(0);
      expect(stepIndexFor(step, "copy")).toBe(0);
    }
  });

  it("maps Media to its own row, and Upload after it", () => {
    expect(stepIndexFor("media", "convert")).toBe(1);
    expect(stepIndexFor("upload", "convert")).toBe(2);
  });

  it("shifts Upload up when there is no Media stage", () => {
    expect(stepIndexFor("upload", "copy")).toBe(1);
  });

  it("never lands an unmapped step on Upload by accident", () => {
    // The old mapping ended in `return 3`, so a step nobody had wired drew
    // its progress on the upload bar.
    expect(stepIndexFor("media", "copy")).toBe(-1);
  });

  it("returns -1, not undefined, for a step string this build doesn't recognise", () => {
    // The event comes off the wire unvalidated. A lookup miss must resolve
    // to "no row"; `undefined` would make every `index < stepIndex` and
    // `index > stepIndex` comparison false at once, marking every row
    // active/done simultaneously.
    const unknownStep = "unknown-step" as unknown as ImportProgressEvent["step"];
    expect(stepIndexFor(unknownStep, "convert")).toBe(-1);
  });
});

describe("stage timing", () => {
  function timeline(events: [ImportProgressEvent["step"], number][]): StageTiming {
    return events.reduce((timing, [step, now]) => recordStageTime(timing, step, now), {
      ...EMPTY_TIMING,
      extractStartedAt: 0,
    });
  }

  it("times each stage from its first event to its last while they overlap", () => {
    // Attachments copy while messages are still read and conversation files
    // are written, so a prepare event must not end the attachment timer.
    const timing = timeline([
      ["setup", 100],
      ["parse", 200],
      ["attachments", 300],
      ["prepare", 400],
      ["parse", 500],
      ["prepare", 700],
      ["attachments", 900],
    ]);
    expect(stageDurations(timing, 1000)).toEqual({
      parseMs: 400,
      attachmentsMs: 600,
      prepareMs: 300,
    });
  });

  it("gives attachments and prepare no time when they never reported", () => {
    const timing = timeline([
      ["parse", 100],
      ["parse", 400],
    ]);
    expect(stageDurations(timing, 1000)).toEqual({ parseMs: 300, attachmentsMs: 0, prepareMs: 0 });
  });

  it("runs reading to the end of extract when only setup reported", () => {
    const timing = timeline([["setup", 100]]);
    expect(stageDurations(timing, 1000).parseMs).toBe(900);
  });
});
