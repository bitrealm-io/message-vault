import { useCallback, useSyncExternalStore } from "react";
import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import { useAuth } from "../../lib/auth";
import type { StagingSummary } from "../../lib/tauri";
import { type ImportPhase, type ImportStep, stepsFor } from "./importProgressState";
import type { ImportJobFormValues } from "./useImportJob";

/** A staging folder Message Crate could not delete, and the reason it gave. */
export type StagingDeleteFailure = { path: string; reason: string };

/**
 * Everything the Import screen shows about the account's one Import Run.
 *
 * It lives here, outside any component, because the run does not stop when
 * the person leaves the Import screen: the desktop keeps staging, converting
 * or uploading on its own thread, and the person is meant to come back to
 * the run wherever it has got to (CONTEXT.md, "Import Run"). React state in
 * the screen was lost on every navigation; this store is read by the screen
 * and by the sidebar badge, and written only by `useImportJob`.
 *
 * The store is named with the account that started the run, the rule ADR 0002
 * sets for every cache entry: another account logged in on the same desktop
 * app reads a fresh form from it, and its actions leave the run alone (#1085).
 * Nothing has to clear it on logout, and the account that started the run
 * finds it again when it logs back in.
 */
export type ImportRunState = {
  /** The account that started the run; null for the fresh form no one has started. */
  accountId: number | null;
  phase: ImportPhase;
  /** True while a stage is doing work, or a probe is running before one. */
  running: boolean;
  steps: ImportStep[];
  /** The settings the run started with; what "what you asked for" shows. */
  form: ImportJobFormValues | null;
  summaryView: ImportSummaryView | null;
  stagingDir: string | null;
  importSessionId: number | null;
  /** What the staging folder held once Staging finished; the Staging row's facts. */
  stagingSummary: StagingSummary | null;
  /**
   * What the folder holds after Media, read again from disk (the folder is
   * the truth, not the last estimate). Null until Media ran.
   */
  mediaSummary: StagingSummary | null;
  /**
   * Files Media tried and could not convert or compress. Null when Media
   * has not run, and on a resume, where the pass's own report is gone.
   */
  mediaFailedCount: number | null;
  mediaToolsMissing: boolean;
  /**
   * True only for a resume that landed at the Staging Review because
   * ffmpeg went missing mid Media, not for the genuine not-yet-run case:
   * the review's copy must not claim Media has not run when it partly has.
   */
  mediaPartiallyRan: boolean;
  /**
   * A resume's own recompute failing (a transient read of the staging
   * folder, not the run itself), surfaced on the resume panel rather than
   * completing the run. Cleared at the start of the next resume attempt or
   * a fresh import; deliberately not cleared by returning to the form,
   * since the failure path returns there itself and still needs it read.
   */
  resumeError: string | null;
  /**
   * The server did not record that the run reached the review on screen.
   * Shown on the review; approving writes the stage again before it goes on.
   */
  reviewError: string | null;
  /**
   * True only while a not-cancellable summarize call is in flight: the
   * review renders once the summary resolves, and until then the run
   * view stays up with Cancel disabled, since there is nothing to stop.
   */
  computingSummary: boolean;
  sourceIdentities: string[] | null;
  /**
   * A staging folder Message Crate could not delete, and why: from a
   * discarded or cancelled run, or the cleanup after a finished one. Shown
   * until the person dismisses it or a later delete of the folder succeeds,
   * so a folder of several gigabytes is never left behind unsaid.
   */
  stagingDeleteFailure: StagingDeleteFailure | null;
};

export function initialImportRunState(steps: ImportStep[]): ImportRunState {
  return {
    accountId: null,
    phase: "form",
    running: false,
    steps,
    form: null,
    summaryView: null,
    stagingDir: null,
    importSessionId: null,
    stagingSummary: null,
    mediaSummary: null,
    mediaFailedCount: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    resumeError: null,
    reviewError: null,
    computingSummary: false,
    sourceIdentities: null,
    stagingDeleteFailure: null,
  };
}

type Patch = Partial<ImportRunState> | ((state: ImportRunState) => Partial<ImportRunState>);

function createImportRunStore(initial: ImportRunState) {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: (): ImportRunState => state,
    set: (patch: Patch): void => {
      const next = typeof patch === "function" ? patch(state) : patch;
      state = { ...state, ...next };
      for (const listener of listeners) listener();
    },
    subscribe: (listener: () => void): (() => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    /** Back to a fresh form: between tests, and when another account takes the store over. */
    reset: (fresh: ImportRunState): void => {
      state = fresh;
      for (const listener of listeners) listener();
    },
  };
}

/** The one store. There is one Import Run per account, and one account logged in. */
export const importRunStore = createImportRunStore(initialImportRunState([]));

/** The fresh form, as an account that has no run in the store reads it. */
const FRESH_RUN = initialImportRunState(stepsFor("copy"));

/**
 * The run in the store when `accountId` started it, and the fresh form when
 * another account did.
 */
export function importRunFor(state: ImportRunState, accountId: number | null): ImportRunState {
  return state.accountId === accountId ? state : FRESH_RUN;
}

/**
 * The logged-in account's run as it stands, re-rendering the caller whenever
 * any of it changes.
 */
export function useImportRunState(): ImportRunState {
  const accountId = useAuth().accountId ?? null;
  const read = useCallback(() => importRunFor(importRunStore.get(), accountId), [accountId]);
  return useSyncExternalStore(importRunStore.subscribe, read, read);
}

/** True at either review: the run is waiting for the person to decide. */
export function isReviewPhase(phase: ImportPhase): boolean {
  return phase === "staging_review" || phase === "media_review";
}
