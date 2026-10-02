import { useMutation } from "@tanstack/react-query";
import { useId, useState } from "react";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import { textInputClassName } from "../../components/TextField";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../../lib/routeQuery";
import { getServerSettings, updateServerSettings } from "../../lib/serverApi";
import { useServerInfo } from "../../lib/useServerInfo";
import { DemoAccountCard } from "./DemoAccountCard";

const MIB = 1024 * 1024;

/** A byte count as the megabytes the field shows: whole when it is whole, two places otherwise. */
function megabytes(bytes: number): string {
  return String(Number((bytes / MIB).toFixed(2)));
}

/**
 * Settings that belong to the whole Message Crate rather than to one account.
 *
 * Public registration is off on a fresh Message Crate, so it admits nobody
 * its owner has not admitted until the owner decides otherwise. The
 * attachment size limit is the largest file the server accepts as an
 * attachment; it lives nowhere but here. The Demo
 * Account is added or reset here. Under them the server states which code it runs and which schema its database
 * carries: the Build, and the Schema Fingerprint as the number the server
 * stamps into the database and names in its startup warning.
 */
export function ServerSettingsPanel() {
  const cache = useRouteCache();
  const { data, isPending, error } = useRouteQuery(keys.serverSettings.all, (signal) =>
    getServerSettings({ signal }),
  );
  const save = useMutation({
    mutationFn: (public_registration: boolean) => updateServerSettings({ public_registration }),
    onSuccess: (settings) => cache.set(keys.serverSettings.all, settings),
    onSettled: () => cache.invalidateAccount(),
  });
  const saveLimit = useMutation({
    mutationFn: (asset_max_bytes: number) => updateServerSettings({ asset_max_bytes }),
    onSuccess: (settings) => {
      cache.set(keys.serverSettings.all, settings);
      setLimitDraft(null);
    },
    onSettled: () => cache.invalidateAccount(),
  });
  // What the owner has typed and not yet saved; null shows the limit in force.
  const [limitDraft, setLimitDraft] = useState<string | null>(null);
  const limitId = useId();
  const info = useServerInfo();

  if (isPending) return <p className="text-[0.875rem] text-muted">Loading settings…</p>;
  if (error) {
    return (
      <p className="text-[0.875rem] text-danger">
        {apiErrorMessage(error, "Could not load server settings.")}
      </p>
    );
  }

  const limitInForce = megabytes(data.asset_max_bytes);
  const limitText = limitDraft ?? limitInForce;
  const typedBytes = Math.round(Number(limitText) * MIB);
  // Compared as text: the field shows the limit rounded to 0.01 MB, so an untouched field must
  // never count as a new limit.
  const canSaveLimit =
    limitDraft !== null &&
    limitDraft !== limitInForce &&
    limitText.trim() !== "" &&
    Number.isFinite(typedBytes) &&
    typedBytes > 0 &&
    typedBytes !== data.asset_max_bytes &&
    !saveLimit.isPending;

  return (
    <section>
      <h3 className="m-0 text-text">Server Settings</h3>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        How this Message Crate behaves, whoever is logged in.
      </p>

      <div className="mt-4 rounded-xl border border-border bg-elevated p-4">
        <Checkbox
          checked={data?.public_registration === true}
          disabled={save.isPending}
          onChange={(checked) => save.mutate(checked)}
        >
          Let anyone who can reach this server create their own account
        </Checkbox>
        <p className="mt-2 text-[0.75rem] text-muted">
          Off: you create every account yourself, and the login screen offers only Login. On: the
          login screen also offers Create Account.
        </p>
        {save.error ? (
          <p className="mt-2 text-[0.813rem] text-danger" role="alert">
            {apiErrorMessage(save.error, "Could not save.")}
          </p>
        ) : null}
      </div>

      <form
        className="mt-4 rounded-xl border border-border bg-elevated p-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSaveLimit) saveLimit.mutate(typedBytes);
        }}
      >
        <label htmlFor={limitId} className="text-[0.875rem] font-semibold text-text">
          Attachment size limit
        </label>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <input
            id={limitId}
            type="number"
            min={1}
            step="any"
            inputMode="decimal"
            className={`${textInputClassName} w-[8rem]`}
            value={limitText}
            disabled={saveLimit.isPending}
            onChange={(event) => setLimitDraft(event.target.value)}
          />
          <span className="text-[0.875rem] text-muted">MB</span>
          <Button type="submit" variant="primary" size="sm" isDisabled={!canSaveLimit}>
            Save
          </Button>
        </div>
        <p className="mt-2 mb-0 text-[0.75rem] text-muted">
          The largest file an import can upload as an attachment. The Staging Review lists the files
          over it. A new limit applies to imports started after it is saved.
        </p>
        {saveLimit.error ? (
          <p className="mt-2 mb-0 text-[0.813rem] text-danger" role="alert">
            {apiErrorMessage(saveLimit.error, "Could not save.")}
          </p>
        ) : null}
      </form>

      <DemoAccountCard />

      {info.data ? (
        <dl className="mt-4 grid grid-cols-[max-content_minmax(0,1fr)] items-baseline gap-x-6 gap-y-2 rounded-xl border border-border bg-elevated p-4 text-[0.875rem]">
          <dt className="text-muted">Version</dt>
          <dd className="m-0 font-mono text-[0.813rem] text-text">{info.data.version}</dd>
          <dt className="text-muted">Schema fingerprint</dt>
          <dd className="m-0 font-mono text-[0.813rem] text-text">
            {info.data.schema_fingerprint}
          </dd>
        </dl>
      ) : null}
    </section>
  );
}
