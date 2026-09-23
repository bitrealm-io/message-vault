import { describe, expect, it } from "vitest";
import type { AttachmentForecast, StagingSummary } from "../../lib/tauri";
import { estimatePiles, estimatesHeading, filesOverLimit, mediaJobVerb } from "./gateForecast";

const MB = 1024 * 1024;

function file(
  name: string,
  sizeMb: number,
  verdict: AttachmentForecast["verdict"],
  estimateMb = sizeMb,
): AttachmentForecast {
  return {
    path: `attachments/${name}`,
    name,
    sizeBytes: sizeMb * MB,
    estimateBytes: estimateMb * MB,
    verdict,
  };
}

function summary(forecasts: AttachmentForecast[]): StagingSummary {
  return {
    conversations: 1,
    messages: 1,
    contactIdentifiers: [],
    ownerHandles: [],
    attachments: forecasts.length,
    attachmentBytes: 0,
    verdictCounts: { fitsAsIs: 0, likelyFits: 0, mayGrow: 0, probablyTooBig: 0, cannotProcess: 0 },
    forecasts,
    assetMaxBytes: 50 * MB,
  };
}

describe("mediaJobVerb", () => {
  it("names the job only for the modes that have a Media stage", () => {
    expect(mediaJobVerb("convert")).toBe("converting");
    expect(mediaJobVerb("compress")).toBe("compressing");
    expect(mediaJobVerb("copy")).toBeNull();
    expect(mediaJobVerb("skip")).toBeNull();
  });
});

describe("filesOverLimit", () => {
  it("keeps only files larger than the limit the summary carries, largest first", () => {
    const over = filesOverLimit(
      summary([
        file("small.mov", 60, "probably_too_big"),
        file("grows.mov", 46, "may_grow", 52),
        file("big.mov", 212, "probably_too_big"),
      ]),
    );
    expect(over.map((row) => row.name)).toEqual(["big.mov", "small.mov"]);
  });

  it("counts a file exactly at the limit as within it", () => {
    expect(filesOverLimit(summary([file("edge.mov", 50, "likely_fits")]))).toEqual([]);
  });
});

describe("estimatePiles", () => {
  it("folds the five verdicts into three piles", () => {
    const piles = estimatePiles(
      summary([
        file("fits.mov", 96, "likely_fits", 38),
        file("huge.mov", 212, "probably_too_big", 84),
        file("grows.mov", 46, "may_grow", 52),
        file("scan.tiff", 71, "cannot_process"),
      ]),
    );
    expect(piles.map((pile) => [pile.label, pile.files.map((row) => row.name)])).toEqual([
      ["Likely within limit", ["fits.mov"]],
      ["May exceed limit", ["huge.mov", "grows.mov"]],
      ["Not audio or video", ["scan.tiff"]],
    ]);
  });

  it("shows no second size for files Media leaves alone", () => {
    const [pile] = estimatePiles(summary([file("scan.tiff", 71, "cannot_process")]));
    expect(pile?.showsEstimate).toBe(false);
  });

  it("drops the piles with nothing in them", () => {
    expect(estimatePiles(summary([]))).toEqual([]);
    const piles = estimatePiles(summary([file("fits.mov", 96, "likely_fits", 38)]));
    expect(piles.map((pile) => pile.key)).toEqual(["likely_within"]);
  });

  it("never says transcode", () => {
    const piles = estimatePiles(
      summary([
        file("a.mov", 96, "likely_fits"),
        file("b.mov", 212, "probably_too_big"),
        file("c.tiff", 71, "cannot_process"),
      ]),
    );
    for (const pile of piles) {
      expect(`${pile.label} ${pile.note}`.toLowerCase()).not.toContain("transcod");
    }
  });
});

describe("estimatesHeading", () => {
  it("names the operation, and is absent when there is no Media stage", () => {
    expect(estimatesHeading("compress")).toBe("Compression estimates");
    expect(estimatesHeading("convert")).toBe("Conversion estimates");
    expect(estimatesHeading("copy")).toBeNull();
    expect(estimatesHeading("skip")).toBeNull();
  });
});
