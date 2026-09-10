import type { ReactNode } from "react";
import Button from "../../components/Button";
import StepProgress from "../../components/StepProgress";
import { formatBytes } from "../../lib/attachmentProgressCopy";
import type { StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import { deltaRows, stillPendingRows } from "./approvalCopy";
import type { GateDelta } from "./gateDelta";
import { forecastGroups, mediaJobVerb, pluralFiles } from "./gateForecast";
import type { ImportStep } from "./importProgressState";
import { type ApprovalKind, approvalSteps } from "./importRunCopy";

const PRIMARY_LABEL: Record<AttachmentMediaMode, string> = {
  convert: "Convert media",
  compress: "Compress media",
  copy: "Upload to vault",
  skip: "Upload to vault",
};

/** Past-tense name of the Media stage's work, for the Media Approval's heading. */
function mediaDoneWord(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "compressed" : "converted";
}

/**
 * An Import Run's approval: the same screen after Staging and after Media.
 *
 * It shows what the staging folder holds, read from the files rather than
 * estimated, and waits for the person to approve the next stage or cancel
 * the run. At the Media Approval it leads with how Media's result differed
 * from what was approved before, since that is the news. Presentational
 * only: every count comes in as a prop, and the decision is the caller's.
 */
export default function ImportApprovalScreen({
  kind,
  steps,
  summary,
  delta,
  unknownContacts,
  mode,
  onApprove,
  onCancel,
  onBack,
  busy,
  mediaToolsMissing,
  mediaPartiallyRan,
  identityPanel,
}: {
  kind: ApprovalKind;
  steps: ImportStep[];
  summary: StagingSummary;
  /** How Media's result differed from the plan; only the Media Approval has one. */
  delta?: GateDelta | null;
  /**
   * Null while the contact-match lookup is in flight or failed. The "new to
   * your vault" clause is a nicety, not a blocker, so a failed lookup omits
   * it rather than stalling the approval.
   */
  unknownContacts: number | null;
  mode: AttachmentMediaMode;
  onApprove: () => void;
  onCancel: () => void;
  /** Back to the run view; the run keeps waiting. */
  onBack: () => void;
  busy?: boolean;
  /**
   * True when convert/compress is selected and ffmpeg was not found:
   * disables approval rather than letting Media fail later.
   */
  mediaToolsMissing?: boolean;
  /**
   * True only when this landing is a resume that found Media partway
   * through (ffmpeg went missing mid pass). The folder may already hold a
   * mix of originals and converted files, so the estimates line would be
   * wrong: it claims Media has not run yet.
   */
  mediaPartiallyRan?: boolean;
  /** The backup's identity list, composed by the caller (omit to hide). */
  identityPanel?: ReactNode;
}) {
  const verb = mediaJobVerb(mode);
  const atStaging = kind === "staging";
  // The breakdown renders whatever the mode: copy/skip has no Media stage,
  // but the exact verdicts (over the limit, not audio or video, …) are
  // still worth surfacing before the person commits to an upload that will
  // drop some of these files.
  const groups = atStaging ? forecastGroups(summary.verdictCounts, mode) : [];
  const toolsBlocked = atStaging && verb != null && Boolean(mediaToolsMissing);
  const primaryLabel = atStaging ? PRIMARY_LABEL[mode] : "Upload to vault";
  const rows = delta ? deltaRows(delta) : [];
  const pendingRows = delta ? stillPendingRows(delta, mode) : [];

  return (
    <>
      <h1 className="m-0 mb-1 text-2xl font-bold">
        {atStaging ? "Approve the staged import" : `Approve the ${mediaDoneWord(mode)} media`}
      </h1>
      <p className="m-0 text-[0.875rem] text-muted">
        {atStaging
          ? "These counts are read from the staged files, so they are exact."
          : `The ${verb ?? "media"} step has finished, so this is where the last check's estimate turned out wrong.`}
      </p>

      <StepProgress steps={approvalSteps(steps, kind)} />

      {!atStaging && delta ? (
        <section className="mt-5">
          <h2 className="m-0 text-base font-semibold">What changed since you approved</h2>
          <div className="mt-3 flex flex-col gap-3">
            {delta.hasChanges ? (
              rows.map((row) => (
                <div key={row.key} className="rounded-lg border border-border p-3">
                  <p className="m-0 text-[0.875rem] font-semibold text-text">{row.text}</p>
                </div>
              ))
            ) : (
              <p className="m-0 text-[0.813rem] text-muted">
                Everything came out as expected — no surprises since you approved.
              </p>
            )}
            {/* Still-pending rows sit alongside the delta, not inside its
                conditional: an import holding only an unconvertible file
                has no delta to report, but the file still will not upload. */}
            {pendingRows.map((row) => (
              <div key={row.key} className="rounded-lg border border-border p-3">
                <p className="m-0 text-[0.875rem] font-semibold text-text">{row.text}</p>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      <div className="mt-5 min-w-0 overflow-hidden rounded-lg border border-border">
        <table className="w-full table-fixed border-collapse text-[0.813rem]">
          <thead>
            <tr className="border-b border-border bg-elevated text-left text-muted">
              <th className="px-3 py-2 font-medium">What was staged</th>
              <th className="w-40 px-3 py-2 text-right font-medium">Count</th>
            </tr>
          </thead>
          <tbody>
            <tr className="border-b border-border">
              <td className="px-3 py-2 text-text">Conversations</td>
              <td className="px-3 py-2 text-right tabular-nums text-text">
                {summary.conversations.toLocaleString()}
              </td>
            </tr>
            <tr className="border-b border-border">
              <td className="px-3 py-2 text-text">Messages</td>
              <td className="px-3 py-2 text-right tabular-nums text-text">
                {summary.messages.toLocaleString()}
              </td>
            </tr>
            <tr className="border-b border-border">
              <td className="px-3 py-2 text-text">Contacts</td>
              <td className="px-3 py-2 text-right tabular-nums text-text">
                {summary.contactIdentifiers.length.toLocaleString()}
                {unknownContacts != null
                  ? ` · ${unknownContacts.toLocaleString()} new to your vault`
                  : ""}
              </td>
            </tr>
            <tr className="border-b border-border">
              <td className="px-3 py-2 text-text">Attachments</td>
              <td className="px-3 py-2 text-right tabular-nums text-text">
                {summary.attachments.toLocaleString()}
              </td>
            </tr>
            <tr className="last:border-b-0">
              <td className="px-3 py-2 text-text">Size staged</td>
              <td className="px-3 py-2 text-right tabular-nums text-text">
                {formatBytes(summary.attachmentBytes)}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      {identityPanel ? (
        <section className="mt-5">
          <h2 className="m-0 text-base font-semibold">Addresses this backup sent from</h2>
          <div className="mt-3">{identityPanel}</div>
        </section>
      ) : null}

      {atStaging && (verb || groups.length > 0) ? (
        <section className="mt-5">
          <h2 className="m-0 text-base font-semibold">
            {verb ? `What to expect after ${verb}` : "Files against the upload limit"}
          </h2>
          <p className="m-0 mt-1 text-[0.813rem] text-muted">
            {verb
              ? mediaPartiallyRan
                ? "The media step needs its tools to finish. Approving here picks up where it left off, once they're available."
                : "The media step has not run yet, so these are estimates based on the files as staged."
              : "There is no media step in this mode, so these sizes are exact, read straight from the staged files."}
          </p>
          <div className="mt-3 flex flex-col gap-3">
            {groups.map((group) => (
              <div key={group.verdict} className="rounded-lg border border-border p-3">
                <p className="m-0 text-[0.875rem] font-semibold text-text">
                  {pluralFiles(group.count)} — {group.label}
                </p>
                <p className="m-0 mt-1 text-[0.813rem] text-muted">{group.hint}</p>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {!atStaging ? (
        <p className="m-0 mt-5 text-[0.813rem] text-muted">
          Messages are always uploaded. A skipped attachment leaves a placeholder in the
          conversation, and the message text is kept. Imported conversations can later be removed
          from your vault in the messages area.
        </p>
      ) : null}

      {toolsBlocked ? (
        <p className="m-0 mt-5 text-[0.813rem] text-muted">
          This step needs ffmpeg. Set its folder in Settings, then come back to Import.
        </p>
      ) : null}

      <div className="mt-5 flex items-center gap-3">
        <Button variant="primary" size="wide" onClick={onApprove} disabled={busy || toolsBlocked}>
          {primaryLabel}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel this import
        </Button>
      </div>
      <div className="mt-4">
        <Button variant="ghost" onClick={onBack} className="!px-3 !py-[0.35rem] !text-[0.875rem]">
          ← Back to the run
        </Button>
      </div>
    </>
  );
}
