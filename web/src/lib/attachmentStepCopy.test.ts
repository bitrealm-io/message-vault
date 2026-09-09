import { describe, expect, it } from "vitest";
import { attachmentStepCopy } from "./attachmentStepCopy";
import type { AttachmentMediaMode } from "./types";

/**
 * This file used to hold three tests, one per branch, each restating the
 * literal in the arm above it. A test written that way cannot fail for any
 * reason a reader would call a defect: change the copy and it goes red, but
 * the copy changing *is* the intended edit, so the only thing it enforces is
 * that someone update two places instead of one.
 *
 * What is worth pinning is the one decision the function makes rather than
 * states: convert and compress share a label, because a person choosing
 * either is watching the same step do the same work, and every mode gets copy
 * of its own so no step is unlabelled.
 */
const MODES: AttachmentMediaMode[] = ["skip", "copy", "convert", "compress"];

describe("attachmentStepCopy", () => {
  it("gives convert and compress the same copy, because they are one step", () => {
    expect(attachmentStepCopy("compress")).toEqual(attachmentStepCopy("convert"));
  });

  it("gives every mode a label and a done detail", () => {
    for (const mode of MODES) {
      const copy = attachmentStepCopy(mode);
      expect(copy.label, `${mode} has a label`).toBeTruthy();
      expect(copy.doneDetail, `${mode} has a done detail`).toBeTruthy();
      expect(copy.label, `${mode}'s label reads as a step`).not.toBe(copy.doneDetail);
    }
  });

  it("keeps skip and copy distinct, which is what the setting chooses between", () => {
    expect(attachmentStepCopy("skip")).not.toEqual(attachmentStepCopy("copy"));
    expect(attachmentStepCopy("skip")).not.toEqual(attachmentStepCopy("convert"));
  });
});
