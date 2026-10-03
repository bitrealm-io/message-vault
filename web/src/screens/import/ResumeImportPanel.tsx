import { useState } from "react";
import Button from "../../components/Button";
import PasswordField from "../../components/PasswordField";
import type { ActiveImportSession } from "../../lib/importSession";
import type { SnapshotSecret } from "./formSnapshot";
import { hintStyle, StackedField } from "./ImportFormUi";
import type { ResumeDecision } from "./resumeDecision";

type ResumableKind = Exclude<ResumeDecision["kind"], "none">;

type PanelCopy = {
  heading: (session: ActiveImportSession) => string;
  body: (session: ActiveImportSession) => string;
  primary: { label: string; action: "resume" | "discard" };
  secondary?: { label: string; action: "discard" };
};

const COPY: Record<ResumableKind, PanelCopy> = {
  resume_push: {
    heading: () => "Finish your last import",
    body: () =>
      "Your messages are staged. Resuming the Upload sends the conversations that are not in your Message Crate yet.",
    primary: { label: "Resume", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  restart: {
    heading: () => "Pick up your last import",
    body: () =>
      "The extract did not finish. Starting again reuses your settings and reads the backup from the beginning.",
    primary: { label: "Start over", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  resume_review: {
    heading: () => "Pick up where you left off",
    body: () =>
      "Your messages are staged. Opening the import again shows you the same summary, read fresh from the folder.",
    primary: { label: "Show me the summary", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  resume_media: {
    heading: () => "Finish preparing your media",
    body: () =>
      "The media step did not finish. Carrying on picks up the files it had not reached yet.",
    primary: { label: "Carry on", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  resume_write: {
    heading: () => "Finish copying your backup",
    body: () =>
      "The copy did not finish. Picking up where you left off reads the backup again and skips the conversations already copied.",
    primary: { label: "Pick up", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  source_changed: {
    heading: () => "The backup has changed",
    body: (session) =>
      session.source_fingerprint?.path
        ? `This import was reading ${session.source_fingerprint.path}, and that backup is different now. Starting over reads it fresh with the same settings.`
        : "The backup this import was reading is different now. Starting over reads it fresh with the same settings.",
    primary: { label: "Start over", action: "resume" },
    secondary: { label: "Discard this import", action: "discard" },
  },
  // resumeDecisionFor routes here both when the staged folder has gone
  // missing and when the session never recorded one — every session created
  // outside the desktop app stores a null staging_dir — so the copy names
  // the path only when there is one.
  folder_missing: {
    heading: (session) =>
      session.staging_dir ? "The staged files are gone" : "There is nothing staged to pick up",
    body: (session) =>
      session.staging_dir
        ? `This import's folder is no longer at ${session.staging_dir}. Discarding it lets you start a new one.`
        : "This import did not record a staged folder, so there is nothing here to carry on from. Discarding it lets you start a new one.",
    primary: { label: "Discard this import", action: "discard" },
  },
  // The stat of the staging folder failed, which says nothing about whether
  // the folder is there, so the copy says that rather than calling it gone.
  folder_unknown: {
    heading: () => "The staged files could not be checked",
    body: (session) =>
      `Message Crate could not check ${session.staging_dir ?? "this import's folder"}. Open Import again to check once more, or discard this import to start a new one.`,
    primary: { label: "Discard this import", action: "discard" },
  },
  other_device: {
    heading: () => "This import belongs to another computer",
    body: () =>
      "It was started on a different install and its files are staged there. Discarding it lets you start a new import here.",
    primary: { label: "Discard this import", action: "discard" },
  },
  settings_unreadable: {
    heading: () => "This import's settings could not be read",
    body: () =>
      "The import is still open here, but the settings it was started with are not readable. Discarding it lets you start a new one.",
    primary: { label: "Discard this import", action: "discard" },
  },
};

// The labels are the Import form's own, so the field reads as the one the
// person filled in when the run started.
const SECRET_COPY: Record<SnapshotSecret, { label: string; hint: string }> = {
  backupPassword: {
    label: "Encryption password",
    hint: "Message Crate does not keep the backup's password. Enter it again to read the backup.",
  },
  whatsappKey: {
    label: "Decryption key",
    hint: "Message Crate does not keep the decryption key. Enter it again to read the backup.",
  },
};

/** Renders one resume decision and calls back on the user's choice. */
export default function ResumeImportPanel({
  decision,
  secret,
  error,
  onResume,
  onDiscard,
}: {
  decision: ResumeDecision;
  /**
   * The password or key the stored run was started with, when acting on
   * this decision reads the backup again. The snapshot never holds the
   * secret itself, so the panel asks for it and holds the resume button
   * until it is filled. Null or absent when there is nothing to ask for.
   */
  secret?: SnapshotSecret | null;
  /**
   * Set when the last attempt to act on this decision failed partway
   * through — today, only a gate/media resume whose recompute of the
   * staged folder failed (a transient read, not a run that actually
   * failed: decision 37 means the session is still here to try again).
   * Null the rest of the time.
   */
  error?: string | null;
  /** Called with what was typed into the secret field, or "" when none was asked for. */
  onResume: (secret: string) => void;
  onDiscard: () => void;
}) {
  const [secretValue, setSecretValue] = useState("");
  const [showSecret, setShowSecret] = useState(false);
  if (decision.kind === "none" || !decision.session) return null;
  const copy = COPY[decision.kind];
  const session = decision.session;
  const resumes = copy.primary.action === "resume";
  const secretCopy = resumes && secret ? SECRET_COPY[secret] : null;

  return (
    <>
      <h1 className="m-0 mb-1 text-2xl font-bold">{copy.heading(session)}</h1>
      <p className="m-0 mb-5 text-[0.875rem] text-muted">{copy.body(session)}</p>
      {error ? (
        <p className="m-0 mb-5 text-[0.813rem] text-danger" role="alert">
          That didn't go through: {error}. You can try again.
        </p>
      ) : null}
      {secretCopy ? (
        <div className="mb-5">
          <StackedField label={secretCopy.label} required>
            <PasswordField
              aria-label={secretCopy.label}
              value={secretValue}
              onChange={setSecretValue}
              autoComplete="new-password"
              showPassword={showSecret}
              onToggle={() => setShowSecret((shown) => !shown)}
            />
            <p className={hintStyle}>{secretCopy.hint}</p>
          </StackedField>
        </div>
      ) : null}
      <div className="flex items-center gap-3">
        <Button
          variant="primary"
          size="wide"
          disabled={secretCopy !== null && secretValue.trim() === ""}
          onClick={resumes ? () => onResume(secretCopy ? secretValue : "") : onDiscard}
        >
          {copy.primary.label}
        </Button>
        {copy.secondary ? (
          <Button variant="ghost" onClick={onDiscard}>
            {copy.secondary.label}
          </Button>
        ) : null}
      </div>
    </>
  );
}
