import { useSyncExternalStore } from "react";
import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import type { StagingSummary } from "../../lib/tauri";
import type { GateDelta } from "./gateDelta";
import type { ImportPhase, ImportStep } from "./importProgressState";
import type { ImportJobFormValues } from "./useImportJob";

/**
 * Everything the Import screen shows about the account's one Import Run.
 *
 * It lives here, outside any component, because the run does not stop when
 * the person leaves the Import screen: the desktop keeps staging, converting
 * or uploading on its own thread, and the person is meant to come back to
 * the run wherever it has got to (CONTEXT.md, "Import Run"). React state in
 * the screen was lost on every navigation; this store is read by the screen
 * and by the sidebar badge, and written only by `useImportJob`.
 */
export type ImportRunState = {
  phase: ImportPhase;
  /** True while a stage is doing work, or a probe is running before one. */
  running: boolean;
  steps: ImportStep[];
  /** The settings the run started with; what "what you asked for" shows. */
  form: ImportJobFormValues | null;
  summaryView: ImportSummaryView | null;
  stagingDir: string | null;
  importSessionId: number | null;
  /**
   * What the staging folder holds, read after Staging and again after Media
   * (the folder is the truth, not the last estimate). Shown at both
   * approvals and on the main screen once Staging is done.
   */
  stagingSummary: StagingSummary | null;
  /** How Media's result differs from what was approved; null until Media ran. */
  mediaDelta: GateDelta | null;
  mediaToolsMissing: boolean;
  /**
   * True only for a resume that landed at the Staging Approval because
   * ffmpeg went missing mid Media, not for the genuine not-yet-run case:
   * the approval's copy must not claim Media has not run when it partly has.
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
   * True only while a not-cancellable summarize call is in flight: the
   * approval renders once the summary resolves, and until then the run
   * view stays up with Cancel disabled, since there is nothing to stop.
   */
  computingSummary: boolean;
  sourceIdentities: string[] | null;
  /**
   * The person clicked "Back to the run" on an approval. The run view then
   * shows the approval as waiting with a button to reopen it, instead of
   * the approval taking the screen again on the next render.
   */
  approvalDismissed: boolean;
};

export function initialImportRunState(steps: ImportStep[]): ImportRunState {
  return {
    phase: "form",
    running: false,
    steps,
    form: null,
    summaryView: null,
    stagingDir: null,
    importSessionId: null,
    stagingSummary: null,
    mediaDelta: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    resumeError: null,
    computingSummary: false,
    sourceIdentities: null,
    approvalDismissed: false,
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
    /** Back to a fresh form. Tests call it between cases; nothing else does. */
    reset: (fresh: ImportRunState): void => {
      state = fresh;
      for (const listener of listeners) listener();
    },
  };
}

/** The one store. There is one Import Run per account, and one account signed in. */
export const importRunStore = createImportRunStore(initialImportRunState([]));

/** The run as it stands, re-rendering the caller whenever any of it changes. */
export function useImportRunState(): ImportRunState {
  return useSyncExternalStore(importRunStore.subscribe, importRunStore.get, importRunStore.get);
}

/** True at either approval: the run is waiting for the person to decide. */
export function isApprovalPhase(phase: ImportPhase): boolean {
  return phase === "staging_approval" || phase === "media_approval";
}
