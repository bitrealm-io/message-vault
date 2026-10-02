import { type ReactNode, Suspense } from "react";
import { Navigate } from "react-router-dom";
import { useAuth } from "../lib/auth";
import {
  type ImportExportBlock,
  type ImportExportFeature,
  importExportBlock,
} from "../lib/desktopFeatures";
import { desktopJobRunningText } from "../lib/desktopJob";
import { isTauri } from "../lib/tauri-check";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useImportRunState } from "../screens/import/importRunStore";
import Button from "./Button";

const TITLE: Record<ImportExportFeature, string> = { import: "Import", export: "Export" };

/**
 * What an account sees in place of the Import or Export form when it does not
 * hold the permission. The Demo Account on Import is sent to the login card,
 * where Create Owner or the login form is; every other case is the Owner's to
 * change.
 */
function PermissionMessage({
  feature,
  block,
}: {
  feature: ImportExportFeature;
  block: ImportExportBlock;
}) {
  const { logout } = useAuth();
  return (
    <div className="max-w-[700px] p-6">
      <h2 className="m-0 mb-6">{TITLE[feature]}</h2>
      {block === "demo" ? (
        <>
          <p className="m-0 text-[0.875rem] text-muted">
            The Demo Account can't import. Importing needs a personal account. Log out to create one
            or to log in to one.
          </p>
          <div className="mt-6">
            <Button variant="primary" size="wide" onClick={() => void logout()}>
              Log out
            </Button>
          </div>
        </>
      ) : (
        <p className="m-0 text-[0.875rem] text-muted">
          The Owner has not allowed this account to {feature}. The Owner sets this under Message
          Permissions in the account's settings.
        </p>
      )}
    </div>
  );
}

/**
 * Import and export stay on the desktop app. There, the route is open to every
 * account: one without the permission gets a message in place of the form, so
 * the form is never filled in only for the server to refuse the run.
 */
export default function ImportExportRoute({
  feature,
  children,
}: {
  feature: ImportExportFeature;
  children: ReactNode;
}) {
  const { profile, loading } = useAccountProfile();
  const run = useImportRunState();
  if (!isTauri()) {
    return <Navigate to="/" replace />;
  }
  if (loading) {
    return null;
  }
  if (profile == null) {
    return <Navigate to="/" replace />;
  }
  const block = importExportBlock(feature, profile);
  // An Import Run this window is already driving stays on screen until it
  // ends, so the person sees how it ended. The server refuses its next request.
  // Once the run is left, the store is back at the form and the message shows.
  if (block && feature === "import" && run.phase !== "form") {
    return (
      <>
        <div
          role="status"
          className="mx-6 mt-6 max-w-[700px] rounded border border-danger-soft-border bg-danger-soft-bg p-3 text-[0.813rem] text-danger"
        >
          The Owner has turned Import off for this account. The server refuses the rest of this
          Import Run, so it ends as failed.
        </div>
        <Suspense fallback={null}>{children}</Suspense>
      </>
    );
  }
  if (block) {
    return <PermissionMessage feature={feature} block={block} />;
  }
  // The desktop runs one job at a time, and an Import Run starts its stages'
  // jobs one after another. An export started between two of them would make
  // the desktop refuse the next stage, so Export waits for the run to end.
  if (feature === "export" && run.running) {
    return (
      <div className="max-w-[700px] p-6">
        <h2 className="m-0 mb-6">{TITLE[feature]}</h2>
        <p role="status" className="m-0 text-[0.875rem] text-muted">
          {desktopJobRunningText("Import Run", TITLE[feature])}
        </p>
      </div>
    );
  }
  // The chunk only starts loading once the route is allowed, so the message
  // and the redirect paths above never pay for it.
  return <Suspense fallback={null}>{children}</Suspense>;
}
