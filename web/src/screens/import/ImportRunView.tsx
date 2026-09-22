import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import VirtualizedImportIssuesTable from "../../components/import/VirtualizedImportIssuesTable";
import OpenPathButton from "../../components/OpenPathButton";
import StepProgress, { type Step } from "../../components/StepProgress";
import { formatBytes } from "../../lib/attachmentProgressCopy";
import { groupSlug } from "../../lib/contactGroups";
import type { AttachmentForecast, StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import { getImport } from "../../lib/vaultApi";
import { useVaultQuery } from "../../lib/vaultQuery";
import ImportContactsPanel from "../settings/storage/ImportContactsPanel";
import { estimatePiles, estimatesHeading, filesOverLimit } from "./gateForecast";
import {
  type ImportPhase,
  type ImportStep,
  MEDIA_LABEL,
  STAGING_LABEL,
  UPLOAD_LABEL,
} from "./importProgressState";
import {
  attachmentsAsked,
  importGroupName,
  type ReviewKind,
  runHeading,
  sourceDisplayName,
} from "./importRunCopy";
import { ExpandableFactRow, FactGroup, FactGroups, FactList, FactRow } from "./RunFacts";
import type { ImportJobFormValues } from "./useImportJob";
import { PUSH_LOG_NAME } from "./useImportJob";

const STAGING_REVIEW_LABEL = "Staging Review";
const MEDIA_REVIEW_LABEL = "Media Review";

/** What approving does next, in the mode's own verb. */
const APPROVE_LABEL: Record<AttachmentMediaMode, string> = {
  convert: "Convert media",
  compress: "Compress media",
  copy: "Upload to vault",
  skip: "Upload to vault",
};

const PATH_LINK =
  "max-w-full border-0 bg-transparent p-0 text-right text-[0.813rem] text-accent underline-offset-2 [overflow-wrap:anywhere] hover:underline";

function count(value: number | undefined | null): string {
  return value == null ? "—" : value.toLocaleString();
}

/** The limit, and the files over it, opened in place with their sizes. */
function AttachmentLimitGroup({ summary }: { summary: StagingSummary }) {
  const over = filesOverLimit(summary);
  return (
    <FactGroup title="Attachments">
      <FactRow label="Size limit per file" value={formatBytes(summary.assetMaxBytes)} />
      {over.length > 0 ? (
        <ExpandableFactRow
          label="Files over the limit"
          caption="Skip vault upload"
          value={over.length.toLocaleString()}
        >
          <FactList
            items={over}
            itemKey={(file) => file.path}
            renderName={(file) => file.name}
            renderValue={(file) => formatBytes(file.sizeBytes)}
          />
        </ExpandableFactRow>
      ) : (
        <FactRow label="Files over the limit" value="0" />
      )}
    </FactGroup>
  );
}

/** Staged size, then what Media is expected to make of it. */
function estimatedSize(file: AttachmentForecast): string {
  return `${formatBytes(file.sizeBytes)} → ${formatBytes(file.estimateBytes)}`;
}

function ReviewActions({
  approveLabel,
  onApprove,
  onCancelRun,
  busy,
  approveDisabled,
}: {
  approveLabel: string;
  onApprove: () => void;
  onCancelRun: () => void;
  busy?: boolean;
  approveDisabled?: boolean;
}) {
  return (
    <div className="mt-1 flex flex-wrap items-center gap-3">
      <Button variant="primary" size="wide" onClick={onApprove} disabled={busy || approveDisabled}>
        {approveLabel}
      </Button>
      <Button onClick={onCancelRun} disabled={busy}>
        Cancel this import
      </Button>
    </div>
  );
}

/** The accent rule down the side of the row that is waiting on the person. */
function WaitingBody({ children }: { children: ReactNode }) {
  return <div className="mt-2 flex flex-col gap-3 border-l-2 border-accent pl-3">{children}</div>;
}

/** What Upload did to the vault's contacts, with the list opened in place. */
function UploadContacts({ importId }: { importId: number }) {
  const detail = useVaultQuery(["imports", importId], (signal) => getImport(importId, { signal }));
  const run = detail.data;
  if (!run) return null;
  const touched = run.contacts_new + run.contacts_changed;
  return (
    <FactGroup title="Contacts" value={touched.toLocaleString()}>
      <FactRow label="New" value={run.contacts_new.toLocaleString()} />
      <FactRow label="Modified" value={run.contacts_changed.toLocaleString()} />
      {touched > 0 ? (
        <ExpandableFactRow label="Contact list">
          <ImportContactsPanel
            importId={importId}
            newCount={run.contacts_new}
            changedCount={run.contacts_changed}
          />
        </ExpandableFactRow>
      ) : null}
    </FactGroup>
  );
}

/** Where the finished run's messages and contacts went. */
function FinishedExits({ importId }: { importId: number }) {
  const navigate = useNavigate();
  const detail = useVaultQuery(["imports", importId], (signal) => getImport(importId, { signal }));
  const run = detail.data;
  const touched = run ? run.contacts_new + run.contacts_changed : 0;
  return (
    <div className="mt-4 flex flex-wrap items-center gap-3">
      <Button
        variant="primary"
        onClick={() => navigate(`/?q=${encodeURIComponent(`import:#${importId}`)}`)}
      >
        View imported conversations
      </Button>
      {run && touched > 0 ? (
        <Button
          onClick={() =>
            navigate(
              `/group/${groupSlug(importGroupName(run.source, run.finished_at, run.started_at))}`,
            )
          }
        >
          View modified contacts
        </Button>
      ) : null}
    </div>
  );
}

/**
 * The Import Run's one screen: the run's stages as a list, each holding what
 * it made, with each Review as a row in that list where the run stops for
 * the person. A finished run leads with where to go next; its errors, and
 * only its errors, sit in a table under the list.
 */
export default function ImportRunView({
  phase,
  steps,
  running,
  form,
  stagingSummary,
  mediaSummary,
  mediaFailedCount,
  summaryView,
  stagingDir,
  importSessionId,
  completionText,
  reviewWaiting,
  unknownContacts,
  mediaToolsMissing,
  mediaPartiallyRan,
  identityPanel,
  reviewBusy,
  onApprove,
  onCancelRun,
  onCancel,
  onBack,
  cancelDisabled,
}: {
  phase: ImportPhase;
  steps: ImportStep[];
  running: boolean;
  form: ImportJobFormValues | null;
  /** What the folder held once Staging finished. */
  stagingSummary: StagingSummary | null;
  /** What the folder holds after Media; null when Media has not run. */
  mediaSummary: StagingSummary | null;
  /** Files Media could not process; null when unknown. */
  mediaFailedCount: number | null;
  summaryView: ImportSummaryView | null;
  stagingDir: string | null;
  importSessionId: number | null;
  completionText?: string;
  /** The review the run is waiting at, when it is. */
  reviewWaiting: ReviewKind | null;
  /**
   * Null while the contact-match lookup is in flight or failed. The split
   * into existing and new is a nicety, not a blocker, so a failed lookup
   * omits it rather than stalling the review.
   */
  unknownContacts: number | null;
  /** Convert or compress is chosen and ffmpeg was not found: approving would only fail later. */
  mediaToolsMissing?: boolean;
  /**
   * This Staging Review is a resume that found Media partway through, so
   * the folder holds a mix of originals and processed files and an estimate
   * of what Media "will" do would be wrong.
   */
  mediaPartiallyRan?: boolean;
  /** The backup's identities, composed by the caller (omit to hide). */
  identityPanel?: ReactNode;
  reviewBusy?: boolean;
  onApprove: () => void;
  /** Cancel the run from a review: the run ends and what was staged is deleted. */
  onCancelRun: () => void;
  /** Stop the stage that is running. */
  onCancel: () => void;
  /** Leave a finished run for the import form. */
  onBack: () => void;
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
  const done = phase === "done";
  const succeeded =
    summaryView?.status === "completed" || summaryView?.status === "completed_with_issues";
  const hasMedia = steps.some((step) => step.label === MEDIA_LABEL);
  const uploadStep = steps.find((step) => step.label === UPLOAD_LABEL);
  const mediaStep = steps.find((step) => step.label === MEDIA_LABEL);

  const cancelButton = running ? (
    <div className="mt-2">
      <Button onClick={onCancel} disabled={cancelDisabled}>
        Cancel
      </Button>
    </div>
  ) : null;

  function stagingContent(): ReactNode {
    if (!trimmedStaging && !stagingSummary && !form) return null;
    return (
      <FactGroups>
        {trimmedStaging ? (
          <FactGroup
            title="Staging directory"
            value={
              <OpenPathButton path={trimmedStaging} className={PATH_LINK}>
                {trimmedStaging}
              </OpenPathButton>
            }
          />
        ) : null}
        {stagingSummary ? (
          <FactGroup title="Conversations" value={stagingSummary.conversations.toLocaleString()}>
            <FactRow label="Messages" value={stagingSummary.messages.toLocaleString()} />
          </FactGroup>
        ) : null}
        {form ? (
          <FactGroup title="Attachments">
            <FactRow label="Action" value={attachmentsAsked(form)} />
            {stagingSummary ? (
              <>
                <FactRow label="Count" value={stagingSummary.attachments.toLocaleString()} />
                <FactRow label="Total size" value={formatBytes(stagingSummary.attachmentBytes)} />
              </>
            ) : null}
          </FactGroup>
        ) : null}
        {form && (form.force || form.obfuscate) ? (
          <FactGroup title="Options">
            {form.force ? <FactRow label="Force reprocessing" value="On" /> : null}
            {form.obfuscate ? <FactRow label="Obfuscate" value="On" /> : null}
          </FactGroup>
        ) : null}
      </FactGroups>
    );
  }

  function stagingReviewContent(summary: StagingSummary): ReactNode {
    const contacts = summary.contactIdentifiers.length;
    const heading = estimatesHeading(mode);
    const piles = heading && !mediaPartiallyRan ? estimatePiles(summary) : [];
    const toolsBlocked = heading != null && Boolean(mediaToolsMissing);
    return (
      <WaitingBody>
        <FactGroups>
          <FactGroup title="Contacts" value={contacts.toLocaleString()}>
            {unknownContacts != null ? (
              <>
                <FactRow label="Existing" value={(contacts - unknownContacts).toLocaleString()} />
                <FactRow label="New" value={unknownContacts.toLocaleString()} />
              </>
            ) : null}
          </FactGroup>
          <AttachmentLimitGroup summary={summary} />
          {heading && piles.length > 0 ? (
            <FactGroup title={heading} caption="Media has not run yet">
              {piles.map((pile) => (
                <ExpandableFactRow
                  key={pile.key}
                  label={pile.label}
                  value={pile.files.length.toLocaleString()}
                >
                  <FactList
                    note={pile.note}
                    items={pile.files}
                    itemKey={(file) => file.path}
                    renderName={(file) => file.name}
                    renderValue={
                      pile.showsEstimate ? estimatedSize : (file) => formatBytes(file.sizeBytes)
                    }
                  />
                </ExpandableFactRow>
              ))}
            </FactGroup>
          ) : null}
          {identityPanel ? (
            <FactGroup title="Identities" caption="Taken from outgoing sent messages">
              {identityPanel}
            </FactGroup>
          ) : null}
        </FactGroups>
        {mediaPartiallyRan ? (
          <p className="m-0 text-[0.813rem] text-muted">
            Media needs its tools to finish. Approving here picks up where it left off, once they're
            available.
          </p>
        ) : null}
        {toolsBlocked ? (
          <p className="m-0 text-[0.813rem] text-muted">
            Media needs ffmpeg. Set its folder in Settings, then come back to Import.
          </p>
        ) : null}
        <ReviewActions
          approveLabel={APPROVE_LABEL[mode]}
          onApprove={onApprove}
          onCancelRun={onCancelRun}
          busy={reviewBusy}
          approveDisabled={toolsBlocked}
        />
      </WaitingBody>
    );
  }

  function mediaContent(summary: StagingSummary): ReactNode {
    return (
      <FactGroups>
        <FactGroup title="Attachments">
          <FactRow label="Total size" value={formatBytes(summary.attachmentBytes)} />
          {mediaFailedCount != null ? (
            <FactRow
              label={mode === "convert" ? "Could not be converted" : "Could not be compressed"}
              value={mediaFailedCount.toLocaleString()}
            />
          ) : null}
        </FactGroup>
      </FactGroups>
    );
  }

  function mediaReviewContent(summary: StagingSummary): ReactNode {
    return (
      <WaitingBody>
        <FactGroups>
          <AttachmentLimitGroup summary={summary} />
        </FactGroups>
        <ReviewActions
          approveLabel="Upload to vault"
          onApprove={onApprove}
          onCancelRun={onCancelRun}
          busy={reviewBusy}
        />
      </WaitingBody>
    );
  }

  function uploadContent(step: ImportStep): ReactNode {
    if (step.status === "pending") return null;
    const finished = done && summaryView != null;
    const skipped =
      summaryView?.messagesParsed != null && summaryView.messagesAttempted != null
        ? summaryView.messagesParsed - summaryView.messagesAttempted
        : 0;
    return (
      <FactGroups>
        {finished ? (
          <>
            <FactGroup title="Messages" value={count(summaryView.messagesAttempted)}>
              <FactRow label="New" value={count(summaryView.messagesInserted)} />
              <FactRow label="Duplicate" value={count(summaryView.messagesDeduped)} />
              <FactRow label="Failed" value={count(summaryView.messagesFailed)} />
              {skipped > 0 ? <FactRow label="Skipped" value={skipped.toLocaleString()} /> : null}
            </FactGroup>
            {summaryView.attachmentsUploaded != null ? (
              <FactGroup title="Attachments">
                <FactRow label="Uploaded" value={count(summaryView.attachmentsUploaded)} />
              </FactGroup>
            ) : null}
            {succeeded && importSessionId != null ? (
              <UploadContacts importId={importSessionId} />
            ) : null}
          </>
        ) : null}
        {logPath ? (
          <FactGroup
            title="Import log"
            value={
              <OpenPathButton path={logPath} title={logPath} className={PATH_LINK}>
                {PUSH_LOG_NAME}
              </OpenPathButton>
            }
          />
        ) : null}
      </FactGroups>
    );
  }

  /** A review's row: waiting, approved once the stage it guards has started, or still ahead. */
  function reviewRow(kind: ReviewKind, guarded: ImportStep | undefined): Step {
    const label = kind === "staging" ? STAGING_REVIEW_LABEL : MEDIA_REVIEW_LABEL;
    if (reviewWaiting === kind) {
      const summary = kind === "staging" ? stagingSummary : mediaSummary;
      return {
        label,
        status: "waiting",
        note: "Awaiting approval",
        content: summary
          ? kind === "staging"
            ? stagingReviewContent(summary)
            : mediaReviewContent(summary)
          : null,
      };
    }
    const approved = guarded != null && guarded.status !== "pending";
    return approved ? { label, status: "done", note: "Approved" } : { label, status: "pending" };
  }

  const rows: Step[] = [];
  for (const step of steps) {
    const active = step.status === "active";
    // A finished stage's facts replace the progress line it ended on.
    const row: Step = { ...step, detail: step.status === "done" ? undefined : step.detail };
    if (step.label === STAGING_LABEL) {
      rows.push({
        ...row,
        content: (
          <>
            {stagingContent()}
            {active ? cancelButton : null}
          </>
        ),
      });
      rows.push(reviewRow("staging", hasMedia ? mediaStep : uploadStep));
    } else if (step.label === MEDIA_LABEL) {
      rows.push({
        ...row,
        content: (
          <>
            {mediaSummary ? mediaContent(mediaSummary) : null}
            {active ? cancelButton : null}
          </>
        ),
      });
      rows.push(reviewRow("media", uploadStep));
    } else {
      rows.push({
        ...row,
        content: (
          <>
            {uploadContent(step)}
            {active ? cancelButton : null}
          </>
        ),
      });
    }
  }

  const hasErrors = done && summaryView != null && summaryView.issues.length > 0;

  return (
    <>
      {done ? (
        <div className="mb-2">
          <Button variant="ghost" size="sm" onClick={onBack}>
            ← Back
          </Button>
        </div>
      ) : null}
      <h1 className="m-0 text-2xl font-bold">
        {runHeading(phase, form, summaryView, completionText)}
      </h1>
      {form ? (
        <p className="m-0 mt-1 text-[0.813rem] text-muted [overflow-wrap:anywhere]">
          {done ? `${sourceDisplayName(form.source)} · ${form.backupPath}` : form.backupPath}
        </p>
      ) : null}

      {done && succeeded && importSessionId != null ? (
        <FinishedExits importId={importSessionId} />
      ) : null}

      <StepProgress steps={rows} wide />
      {/* Running with no stage active (the summary is being read): Cancel has no row to sit in. */}
      {steps.some((step) => step.status === "active") ? null : cancelButton}

      {hasErrors ? (
        <section className="mt-6 min-w-0 overflow-hidden border-t border-border pt-4">
          <h2 className="m-0 text-[0.875rem] font-semibold text-text">
            Errors
            <span className="ml-1.5 font-normal tabular-nums text-muted">
              {summaryView.issues.length.toLocaleString()}
            </span>
          </h2>
          <p className="m-0 mt-1 text-[0.75rem] text-muted">
            Identical errors are grouped. Click a row for the full message and the files it names.
          </p>
          <VirtualizedImportIssuesTable issues={summaryView.issues} />
        </section>
      ) : null}
    </>
  );
}
