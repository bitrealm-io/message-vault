import type { ReactNode } from "react";

/** `waiting` is a row stopped for the person to decide, not one doing work. */
export type StepStatus = "pending" | "active" | "waiting" | "done" | "error";

export type Step = {
  label: string;
  status: StepStatus;
  /** Status line under the label (e.g. "Extraction complete"). */
  detail?: string;
  /** Wall-clock duration for a finished step, shown beside the label. */
  durationMs?: number | null;
  /** Short word beside the label where a duration would go (e.g. "Approved"). */
  note?: string;
  /** What the step made or is asking, rendered under its label. Wide lists only. */
  content?: ReactNode;
};

type StepProgressProps = {
  steps: Step[];
  /** Status line shown under the list once every step has finished. */
  completionText?: ReactNode;
  /**
   * Fill the width and put each step's duration at the right edge of its
   * label line, so a step's `content` can run the full width under it.
   */
  wide?: boolean;
};

function formatStepDuration(milliseconds: number): string {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  if (minutes > 0) return `${minutes}m ${remainingSeconds}s`;
  return `${remainingSeconds}s`;
}

function stepLabelClass(status: StepStatus): string {
  if (status === "active" || status === "waiting") return "font-semibold text-text";
  if (status === "pending") return "text-muted";
  return "text-text";
}

function StepGlyph({ status, index }: { status: StepStatus; index: number }) {
  const slot = "flex h-6 w-6 shrink-0 items-center justify-center";

  if (status === "active") {
    return (
      <span className={slot} aria-hidden>
        <span className="h-4 w-4 animate-spin rounded-full border-2 border-accent border-t-transparent" />
      </span>
    );
  }
  if (status === "waiting") {
    return (
      <span className={slot} aria-hidden>
        <span className="h-2.5 w-2.5 rounded-full bg-accent ring-4 ring-accent/30" />
      </span>
    );
  }
  if (status === "done") {
    return (
      <span className={`${slot} text-[1rem] font-semibold text-ok`} aria-hidden>
        ✓
      </span>
    );
  }
  if (status === "error") {
    return (
      <span className={`${slot} text-[1rem] font-semibold text-danger`} aria-hidden>
        !
      </span>
    );
  }
  return (
    <span className={`${slot} text-[0.75rem] font-semibold text-muted`} aria-hidden>
      {index + 1}
    </span>
  );
}

type CompletionKind = "ok" | "warn" | "error" | "muted";

function completionKind(text: ReactNode): CompletionKind {
  if (text === "Import complete") return "ok";
  if (text === "Import completed with issues") return "warn";
  if (text === "Import failed") return "error";
  return "muted";
}

function completionBadgeClass(kind: CompletionKind): string {
  if (kind === "ok") return "bg-ok text-sent-text";
  if (kind === "warn") return "bg-warn-soft-bg text-warn-soft-text";
  if (kind === "error") return "bg-danger text-sent-text";
  return "bg-border text-muted";
}

function completionBadgeMark(kind: CompletionKind): string {
  if (kind === "ok") return "✓";
  if (kind === "warn") return "!";
  if (kind === "error") return "!";
  return "–";
}

/**
 * Ordered list of import steps. Each step is an <li>; the active step carries
 * aria-current="step" so assistive tech announces where the job is.
 */
export default function StepProgress({ steps, completionText, wide }: StepProgressProps) {
  const kind = completionText == null ? null : completionKind(completionText);

  if (wide) {
    return (
      <ol className="m-0 mt-6 flex w-full list-none flex-col gap-4 p-0">
        {steps.map((step, i) => {
          const current = step.status === "active" || step.status === "waiting";
          const aside = step.durationMs != null ? formatStepDuration(step.durationMs) : step.note;
          return (
            <li
              key={step.label}
              aria-current={current ? "step" : undefined}
              className="grid grid-cols-[1.5rem_minmax(0,1fr)] items-start gap-x-2"
            >
              <StepGlyph status={step.status} index={i} />
              <div className="min-w-0">
                <div
                  className={`flex items-baseline justify-between gap-4 text-[0.875rem] ${stepLabelClass(step.status)}`}
                >
                  <span>{step.label}</span>
                  {aside ? (
                    <span className="text-[0.813rem] font-normal tabular-nums text-muted">
                      {aside}
                    </span>
                  ) : null}
                </div>
                {step.detail ? (
                  <div className="mt-0.5 text-[0.75rem] text-muted">{step.detail}</div>
                ) : null}
                {step.content}
              </div>
            </li>
          );
        })}
      </ol>
    );
  }

  return (
    <div className="mt-6">
      <ol className="m-0 grid w-fit max-w-full grid-cols-[1.5rem_minmax(0,max-content)_max-content] gap-x-2 gap-y-3 p-0">
        {steps.map((step, i) => (
          <li
            key={step.label}
            aria-current={step.status === "active" ? "step" : undefined}
            className="col-span-3 grid grid-cols-subgrid items-start"
          >
            <StepGlyph status={step.status} index={i} />
            <div className="min-w-0 max-w-[min(36rem,70vw)]">
              <div className={`text-[0.875rem] ${stepLabelClass(step.status)}`}>{step.label}</div>
              {step.detail ? (
                <div className="mt-0.5 text-[0.75rem] text-muted">{step.detail}</div>
              ) : null}
            </div>
            {step.durationMs != null ? (
              <span className="pt-0.5 text-[0.813rem] tabular-nums text-muted">
                {formatStepDuration(step.durationMs)}
              </span>
            ) : (
              <span />
            )}
          </li>
        ))}
      </ol>
      {completionText != null && kind != null ? (
        <div className="mt-5 flex items-center gap-2">
          <span
            className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[0.75rem] font-semibold ${completionBadgeClass(kind)}`}
            aria-hidden
          >
            {completionBadgeMark(kind)}
          </span>
          <p className="mb-0 text-[0.875rem] font-medium text-text">{completionText}</p>
        </div>
      ) : null}
    </div>
  );
}
