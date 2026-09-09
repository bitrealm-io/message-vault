import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import ImportSummaryPanel, {
  type ImportSummaryView,
} from "../../components/import/ImportSummaryPanel";
import OpenPathButton from "../../components/OpenPathButton";
import StepProgress from "../../components/StepProgress";
import { formatBytes } from "../../lib/attachmentProgressCopy";
import { groupSlug } from "../../lib/contactGroups";
import type { StagingSummary } from "../../lib/tauri";
import { getImport } from "../../lib/vaultApi";
import { useVaultQuery } from "../../lib/vaultQuery";
import ImportContactsPanel from "../settings/storage/ImportContactsPanel";
import { deltaRows, stillPendingRows } from "./approvalCopy";
import type { GateDelta } from "./gateDelta";
import type { ImportPhase, ImportStep } from "./importProgressState";
import {
  type ApprovalKind,
  attachmentsAsked,
  importGroupName,
  runHeading,
  sourceDisplayName,
} from "./importRunCopy";
import type { ImportJobFormValues } from "./useImportJob";
import { PUSH_LOG_NAME } from "./useImportJob";

function ResultRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-0.5 text-[0.813rem]">
      <span className="text-muted">{label}</span>
      <span className="tabular-nums text-text">{value}</span>
    </div>
  );
}

function ResultBlock({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-4 rounded-lg border border-border p-3">
      <h2 className="m-0 mb-1 text-[0.875rem] font-semibold text-text">{title}</h2>
      {children}
    </section>
  );
}

/**
 * Where the finished run's messages and contacts went, with the run's
 * contact list and what the run did to each contact.
 */
function FinishedExits({
  importId,
  onImportAnother,
}: {
  importId: number;
  onImportAnother: () => void;
}) {
  const navigate = useNavigate();
  const detail = useVaultQuery(["imports", importId], (signal) => getImport(importId, { signal }));
  const run = detail.data;
  const contactsTouched = run ? run.contacts_new + run.contacts_changed : 0;
  return (
    <>
      {run ? (
        <ResultBlock title="Contacts">
          <ImportContactsPanel
            importId={importId}
            newCount={run.contacts_new}
            changedCount={run.contacts_changed}
          />
        </ResultBlock>
      ) : null}
      <div className="mt-5 flex flex-wrap items-center gap-3">
        <Button
          variant="primary"
          onClick={() => navigate(`/?q=${encodeURIComponent(`import:#${importId}`)}`)}
        >
          Conversations this import added
        </Button>
        {run && contactsTouched > 0 ? (
          <Button
            onClick={() =>
              navigate(
                `/group/${groupSlug(importGroupName(run.source, run.finished_at, run.started_at))}`,
              )
            }
          >
            Contacts it touched
          </Button>
        ) : null}
        <Button variant="ghost" onClick={onImportAnother}>
          Import another
        </Button>
      </div>
    </>
  );
}

/**
 * The Import Run's main screen: what the person asked for, the three
 * stages as they run, and each stage's result underneath as it lands.
 * When the run is waiting at an approval, this shows it waiting and offers
 * to open the approval; when it has finished, it leads with where to go
 * next.
 */
export default function ImportRunView({
  phase,
  steps,
  running,
  form,
  stagingSummary,
  mediaDelta,
  summaryView,
  stagingDir,
  importSessionId,
  completionText,
  approvalWaiting,
  onCancel,
  onReview,
  onImportAnother,
  cancelDisabled,
}: {
  phase: ImportPhase;
  steps: ImportStep[];
  running: boolean;
  form: ImportJobFormValues | null;
  stagingSummary: StagingSummary | null;
  mediaDelta: GateDelta | null;
  summaryView: ImportSummaryView | null;
  stagingDir: string | null;
  importSessionId: number | null;
  completionText?: string;
  /** The approval the run is waiting at, when it is. */
  approvalWaiting: ApprovalKind | null;
  onCancel: () => void;
  /** Open the waiting approval. */
  onReview: () => void;
  onImportAnother: () => void;
  /**
   * True while a not-cancellable step (recomputing the staging summary) is
   * running: Cancel stays visible, since a running job is still shown, but
   * disabled rather than offered as a control with nothing to stop.
   */
  cancelDisabled?: boolean;
}) {
  const trimmedStaging = stagingDir?.trim() || null;
  const logPath = trimmedStaging ? `${trimmedStaging}/${PUSH_LOG_NAME}` : null;
  const mode = form?.attachmentMedia ?? "copy";
  const rows = mediaDelta ? deltaRows(mediaDelta) : [];
  const pendingRows = mediaDelta ? stillPendingRows(mediaDelta, mode) : [];
  const done = phase === "done";

  return (
    <>
      <h1 className="m-0 mb-4 text-2xl font-bold">
        {runHeading(phase, form, summaryView, completionText)}
      </h1>

      {form ? (
        <ResultBlock title="What you asked for">
          <ResultRow label="Source" value={sourceDisplayName(form.source)} />
          <ResultRow label="Backup" value={form.backupPath} />
          <ResultRow label="Attachments" value={attachmentsAsked(form)} />
          {form.force ? <ResultRow label="Force reprocessing" value="On" /> : null}
          {form.obfuscate ? <ResultRow label="Obfuscate" value="On" /> : null}
          {trimmedStaging && logPath ? (
            <div className="mt-2 border-t border-border pt-2 text-[0.813rem]">
              <div>
                <span className="text-muted">Staging directory</span>
                <div className="mt-0.5">
                  <OpenPathButton
                    path={trimmedStaging}
                    className="max-w-full truncate border-0 bg-transparent p-0 text-left text-[0.813rem] text-accent underline-offset-2 hover:underline"
                  >
                    {trimmedStaging}
                  </OpenPathButton>
                </div>
              </div>
              <div className="mt-2 border-l border-border pl-3">
                <span className="text-muted">Import log</span>
                <div className="mt-0.5">
                  <OpenPathButton
                    path={logPath}
                    title={logPath}
                    className="border-0 bg-transparent p-0 text-left text-[0.813rem] text-accent underline-offset-2 hover:underline"
                  >
                    {PUSH_LOG_NAME}
                  </OpenPathButton>
                </div>
              </div>
            </div>
          ) : null}
        </ResultBlock>
      ) : null}

      <StepProgress steps={steps} completionText={completionText} />

      {running ? (
        <div className="mt-4">
          <Button onClick={onCancel} disabled={cancelDisabled}>
            Cancel
          </Button>
        </div>
      ) : null}

      {stagingSummary ? (
        <ResultBlock title="Staging">
          <ResultRow label="Conversations" value={stagingSummary.conversations.toLocaleString()} />
          <ResultRow label="Messages" value={stagingSummary.messages.toLocaleString()} />
          <ResultRow
            label="Attachments"
            value={`${stagingSummary.attachments.toLocaleString()} · ${formatBytes(stagingSummary.attachmentBytes)}`}
          />
        </ResultBlock>
      ) : null}

      {mediaDelta ? (
        <ResultBlock title="Media">
          {mediaDelta.hasChanges ? (
            rows.map((row) => (
              <p key={row.key} className="m-0 py-0.5 text-[0.813rem] text-text">
                {row.text}
              </p>
            ))
          ) : (
            <p className="m-0 py-0.5 text-[0.813rem] text-muted">
              Everything came out as expected.
            </p>
          )}
          {pendingRows.map((row) => (
            <p key={row.key} className="m-0 py-0.5 text-[0.813rem] text-text">
              {row.text}
            </p>
          ))}
        </ResultBlock>
      ) : null}

      {approvalWaiting ? (
        <section className="mt-4 rounded-lg border border-accent p-3">
          <h2 className="m-0 text-[0.875rem] font-semibold text-text">
            {approvalWaiting === "staging" ? "Staging Approval" : "Media Approval"} · waiting for
            you
          </h2>
          <p className="m-0 mt-1 text-[0.813rem] text-muted">
            The run does nothing more until you approve it or cancel it.
          </p>
          <div className="mt-3">
            <Button variant="primary" onClick={onReview}>
              Review and approve
            </Button>
          </div>
        </section>
      ) : null}

      {done && summaryView ? (
        <>
          <ImportSummaryPanel summary={summaryView} embedStepTimings={false} />
          {importSessionId != null &&
          (summaryView.status === "completed" || summaryView.status === "completed_with_issues") ? (
            <FinishedExits importId={importSessionId} onImportAnother={onImportAnother} />
          ) : (
            <div className="mt-4">
              <Button variant="primary" onClick={onImportAnother} size="wide">
                Import another
              </Button>
            </div>
          )}
        </>
      ) : null}
    </>
  );
}
