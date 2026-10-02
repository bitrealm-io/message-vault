import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../../lib/routeQuery";
import { parseSelectKey } from "../../lib/selectKey";
import { getDemoAccount, replaceDemoAccount } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";

type DemoDataSize = components["schemas"]["DemoDataSize"];

const SIZES = ["medium", "large"] as const;

const SIZE_LABEL: Record<DemoDataSize, string> = {
  medium: "Medium, about 54,000 messages",
  large: "Large, about 613,000 messages",
};

/** How often the card asks the server whether a build has ended. */
const BUILD_POLL_MS = 1500;

/**
 * Add the Demo Account, or reset it.
 *
 * The server builds it after it answers and keeps serving meanwhile, so the
 * card reads the Demo Account every moment or two while a build runs and
 * shows that it is working. A reset removes everything a visitor changed, so
 * it asks first; adding does not, since there is nothing to lose.
 */
export function DemoAccountCard() {
  const cache = useRouteCache();
  const demo = useRouteQuery(keys.demoAccount.all, (signal) => getDemoAccount({ signal }), {
    refetchInterval: (query) => (query.state.data?.status === "building" ? BUILD_POLL_MS : false),
    // A build outlasts a glance at another tab, and the page should be right
    // when the owner comes back to it.
    refetchIntervalInBackground: true,
  });
  const [size, setSize] = useState<DemoDataSize>("medium");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const build = useMutation({
    mutationFn: (chosen: DemoDataSize) => replaceDemoAccount({ size: chosen }),
    onSuccess: (started) => {
      cache.set(keys.demoAccount.all, started);
      setConfirmOpen(false);
    },
    onSettled: () => cache.invalidateAccount(),
  });

  // When a build ends, the account list, the storage counts and the login
  // card's button all changed with it. The build is a write the server
  // finishes on its own, so its end marks the account's cache stale as a
  // write's settling does.
  const status = demo.data?.status;
  const wasBuilding = useRef(false);
  useEffect(() => {
    if (wasBuilding.current && status !== undefined && status !== "building") {
      cache.invalidateAccount();
    }
    wasBuilding.current = status === "building";
  }, [status, cache]);

  if (demo.isPending) return null;
  if (demo.error || !demo.data) {
    return (
      <p className="mt-4 text-[0.875rem] text-danger">
        {apiErrorMessage(demo.error, "Could not load the Demo Account.")}
      </p>
    );
  }

  const building = status === "building";
  const exists = status === "ready";
  const start = () => {
    if (exists) setConfirmOpen(true);
    else build.mutate(size);
  };

  return (
    <div className="mt-4 rounded-xl border border-border bg-elevated p-4">
      <h4 className="m-0 text-[0.875rem] font-semibold text-text">Demo Account</h4>
      <p className="mt-1 mb-3 text-[0.75rem] text-muted">
        Made-up conversations anyone who reaches this server can open from the login screen, with no
        password. It can't import or delete for good.{" "}
        {exists
          ? "Resetting removes everything visitors changed in it. To remove it, delete it under User Accounts."
          : null}
        {status === "absent" ? "This Message Crate has none." : null}
      </p>

      {building ? (
        <p className="m-0 text-[0.875rem] text-text" role="status">
          Building the Demo Account
          {demo.data.size ? ` (${SIZE_LABEL[demo.data.size].toLowerCase()})` : ""}… The server keeps
          working meanwhile, and this page updates when it is done.
        </p>
      ) : (
        <div className="flex flex-wrap items-center gap-3">
          <Select
            selectedKey={size}
            aria-label="Demo Data size"
            className="w-[19rem]"
            isDisabled={build.isPending}
            onSelectionChange={(key) => {
              const next = parseSelectKey(key, SIZES);
              if (next) setSize(next);
            }}
          >
            {SIZES.map((option) => (
              <ListBoxItem key={option} id={option} className={selectItemClassName}>
                {SIZE_LABEL[option]}
              </ListBoxItem>
            ))}
          </Select>
          <Button variant="primary" size="sm" isDisabled={build.isPending} onPress={start}>
            {exists ? "Reset Demo Account" : "Add Demo Account"}
          </Button>
        </div>
      )}
      {!building && size === "large" ? (
        <p className="mt-2 mb-0 text-[0.75rem] text-muted">The large set takes about a minute.</p>
      ) : null}

      {status === "failed" ? (
        <p className="mt-2 mb-0 text-[0.813rem] text-danger" role="alert">
          The last build failed, and the Demo Account was removed: {demo.data.error}
        </p>
      ) : null}
      {build.error && !confirmOpen ? (
        <p className="mt-2 mb-0 text-[0.813rem] text-danger" role="alert">
          {apiErrorMessage(build.error, "Could not start the build.")}
        </p>
      ) : null}

      <ConfirmDialog
        open={confirmOpen}
        title="Reset the Demo Account?"
        body="This removes the Demo Account with everything visitors changed in it, and builds it again. No other account is touched."
        confirmLabel="Reset Demo Account"
        busy={build.isPending}
        error={build.error ? apiErrorMessage(build.error, "Could not start the build.") : ""}
        onClose={() => setConfirmOpen(false)}
        onConfirm={() => build.mutate(size)}
      />
    </div>
  );
}
