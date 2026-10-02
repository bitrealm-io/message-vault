import { useCallback, useEffect, useRef, useState } from "react";
import Button from "../components/Button";
import { setBaseUrl } from "../lib/api";
import { useAuth } from "../lib/auth";
import { DEFAULT_TAURI_SERVER_URL, initialLoginServerUrl } from "../lib/authGuards";
import { isOwnAddress, type LocalServerStatus, openDataFolder } from "../lib/localServer";
import { checkServerHealth, type ServerHealthStatus } from "../lib/serverHealth";
import { isTauri } from "../lib/tauri-check";
import { accentLink, authCard, authCardBody, authScreenTitle, pageCenter } from "../lib/uiStyles";
import { useLocalServer } from "../lib/useLocalServer";
import { useServerHealth } from "../lib/useServerHealth";
import { useServerState } from "../lib/useServerState";
import ExploreDemoAccountButton from "./auth/ExploreDemoAccountButton";
import LocalAuthTabs from "./auth/LocalAuthTabs";
import ServerSettingsScreen from "./auth/ServerSettingsScreen";
import ServerStatus, { type ServerConnection } from "./auth/ServerStatus";

/** Placeholder shaped like the form, so the card does not flicker into shape. */
function FormSkeleton() {
  return (
    <div className="min-h-0 flex-1" aria-hidden="true" data-testid="auth-form-skeleton">
      <div className="mb-6 h-9 rounded bg-elevated" />
      <div className="h-3.5 w-1/3 rounded bg-elevated" />
      <div className="mt-2 h-10 rounded bg-elevated" />
      <div className="mt-5 h-3.5 w-1/4 rounded bg-elevated" />
      <div className="mt-2 h-10 rounded bg-elevated" />
    </div>
  );
}

/** Hairline either side of the word, parting the way in from the way out. */
function OrRule() {
  return (
    <div className="mt-2.5 flex items-center gap-3 text-[0.75rem] text-muted">
      <span className="h-px flex-1 bg-border" />
      or
      <span className="h-px flex-1 bg-border" />
    </div>
  );
}

/** What the card says while the desktop app starts its own Message Crate. */
function startingLabel(status: LocalServerStatus | null): string | undefined {
  if (status?.status !== "starting") return undefined;
  return status.first_time
    ? "Setting up Message Crate for the first time…"
    : "Starting Message Crate…";
}

/**
 * What the card shows in place of its forms when the desktop app could not
 * start its own Message Crate: one sentence, the ways on, and the server's
 * own words for a bug report. Change server address stays where it always
 * is, under the card.
 */
function StartFailed({
  status,
  onRetry,
}: {
  status: Extract<LocalServerStatus, { status: "failed" }>;
  onRetry: () => void;
}) {
  const [openError, setOpenError] = useState<string | null>(null);
  return (
    <div className="min-h-0 flex-1">
      <p className="m-0 text-[0.875rem] text-text" role="alert">
        {status.message}
      </p>
      <div className="mt-4 grid grid-cols-2 gap-2.5">
        <Button variant="primary" onPress={onRetry}>
          Try again
        </Button>
        <Button
          variant="secondary"
          onPress={() => {
            setOpenError(null);
            openDataFolder().catch((caught: unknown) => {
              setOpenError(caught instanceof Error ? caught.message : String(caught));
            });
          }}
        >
          Open data folder
        </Button>
      </div>
      {openError ? <p className="m-0 mt-1 text-[0.75rem] text-danger">{openError}</p> : null}
      {status.details ? (
        <details className="mt-4 text-[0.75rem] text-muted">
          <summary className="cursor-pointer">Details</summary>
          <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-elevated p-2 font-mono text-[0.7rem] text-text">
            {status.details}
          </pre>
        </details>
      ) : null}
    </div>
  );
}

/**
 * The way into a Message Crate. The card resolves an address on mount and confirms the
 * server is reachable itself, so the only question the old first screen asked —
 * which server — is answered by default, reported as a single word under the
 * product name, and changed on a settings screen when the default is wrong.
 */
export default function LoginScreen() {
  const { setServer: setAuthServer, serverUrl: savedUrl, retrySavedLogin } = useAuth();
  const [address, setAddress] = useState(() => initialLoginServerUrl(savedUrl, isTauri()));
  const [draft, setDraft] = useState(address);
  const [state, setState] = useState<ServerConnection>("connecting");
  // Sticky once true: once the login form has been shown, keep showing it
  // (dimmed while disconnected) instead of reverting to the skeleton.
  const [hasConnectedOnce, setHasConnectedOnce] = useState(false);
  // The desktop app opens on this card only for its own Message Crate. An
  // address the person entered is theirs to confirm at every start, and the
  // connection screen is also the way back to the app's own.
  const [settingsOpen, setSettingsOpen] = useState(() => isTauri() && !isOwnAddress(address));
  // True until that first connection screen is left: the address it shows is
  // already the saved one, and using it as it stands is a real choice there.
  const [confirmingAtStart, setConfirmingAtStart] = useState(settingsOpen);
  // What Test reported for the address currently typed, or null when it has
  // not been tested since the last edit.
  const [tested, setTested] = useState<ServerConnection | null>(null);

  // A disconnected card keeps checking the address it already has, so it can
  // heal itself the moment the server comes back. Nothing else probes: the
  // settings screen asks explicitly, with Test.
  const health = useServerHealth(state === "disconnected" ? address : null);

  // The desktop app starts a Message Crate of its own at its own address and
  // nowhere else. The browser, and any address the person entered, start
  // nothing.
  const { status: localServer, retry: retryLocalServer } = useLocalServer(
    isTauri() && isOwnAddress(address),
  );
  const localStarting = localServer?.status === "starting";
  const localFailed = localServer?.status === "failed" ? localServer : null;

  // Which forms this card offers is the server's answer, not a guess made here.
  // Asked only once the address is reachable, so an unreachable server reports
  // "disconnected" rather than a failed state query.
  const { state: serverState, demoAccount } = useServerState(
    state === "connected" ? address : null,
  );

  // Two connects can be in flight at once — the background self-heal for the
  // address already saved, and the explicit reconnect for one just typed — so
  // only the newest may write. An earlier slow probe that lands second would
  // otherwise put its own address back in the box, and the address the person
  // typed would disappear in front of them.
  const connectRun = useRef(0);
  const connectAbort = useRef<AbortController | null>(null);
  const connect = useCallback(
    async (url: string) => {
      const trimmed = url.trim();
      const run = connectRun.current + 1;
      connectRun.current = run;
      // The superseded probe has nothing left to say, so stop waiting on it
      // rather than holding the request open until it times out.
      connectAbort.current?.abort();
      const controller = new AbortController();
      connectAbort.current = controller;
      setState("connecting");
      setBaseUrl(trimmed);
      // GET /health answers plain text, not JSON, so this probes it directly
      // rather than through apiClient (which always parses the body as
      // JSON). The body is discarded either way — only reachability matters.
      const reachable = await checkServerHealth(trimmed, controller.signal);
      if (connectRun.current !== run) return;
      if (reachable) {
        setAddress(trimmed);
        setDraft(trimmed);
        setHasConnectedOnce(true);
        setAuthServer(trimmed);
        setState("connected");
      } else {
        // Nothing answered. That is the status line's problem, not the form's.
        setState("disconnected");
      }
    },
    [setAuthServer],
  );

  // Resolve the server once on mount; Change server address calls `connect` again.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void connect(address);
  }, [connect, address]);

  // A disconnected card heals itself: when the live health probe finds the
  // server reachable again, reconnect without waiting to be asked. Fires only on
  // the transition into "ok" — not on every render while it stays "ok" — so a
  // `connect()` that fails and lands back in "disconnected" does not
  // immediately retry.
  const previousHealth = useRef<ServerHealthStatus>(health);
  useEffect(() => {
    const becameHealthy = previousHealth.current !== "ok" && health === "ok";
    previousHealth.current = health;
    if (state === "disconnected" && becameHealthy) {
      void connect(address);
      // A login saved before the server went quiet was never rejected, so it
      // is checked now; a server that accepts it takes the person straight in.
      retrySavedLogin(address);
    }
  }, [health, state, address, connect, retrySavedLogin]);

  // The moment the app's own server answers, connect, rather than wait for
  // the health probe's next turn.
  const localReady = localServer?.status === "ready";
  const previousLocalReady = useRef(false);
  useEffect(() => {
    const becameReady = !previousLocalReady.current && localReady;
    previousLocalReady.current = localReady;
    if (becameReady && state !== "connected") void connect(address);
  }, [localReady, state, address, connect]);

  // Only the newest Test may write the result: an earlier slow probe must not
  // stamp its answer over a later one, or over a screen that has since closed.
  const testRun = useRef(0);
  const runTest = useCallback(async () => {
    const run = testRun.current + 1;
    testRun.current = run;
    setTested("connecting");
    const reachable = await checkServerHealth(draft.trim());
    if (testRun.current !== run) return;
    setTested(reachable ? "connected" : "disconnected");
  }, [draft]);

  /**
   * What the settings screen reports under Connection Status.
   *
   * Test's answer wins while it lasts. Without one, the card's own connection
   * may be shown only while the box still holds the address that connection
   * was made to — edit a character and `state` is describing a different
   * server, so repeating it here would tell the person that the address they
   * are typing works, on the strength of a probe that never touched it. That
   * is how a failed Test used to turn green again on the next keystroke.
   */
  const trimmedDraft = draft.trim();
  const settingsStatus: ServerConnection =
    tested ?? (trimmedDraft === address ? state : "untested");

  // Use this address applies an address. An empty field names no address,
  // and the one already connected is not a change: applying it would drop the
  // card back to "connecting", re-probe the same server, and land where it
  // started. Either way there is nothing to apply, so the button is disabled
  // until the field holds a different address.
  // The one exception is the connection screen the desktop app opens on:
  // there the saved address is the one being offered.
  const canApplyDraft = trimmedDraft !== "" && (trimmedDraft !== address || confirmingAtStart);

  const closeSettings = () => {
    testRun.current += 1;
    setTested(null);
    setSettingsOpen(false);
    setConfirmingAtStart(false);
  };

  return (
    <div className={pageCenter}>
      <div className={authCard}>
        <div className={authCardBody}>
          {settingsOpen ? (
            <ServerSettingsScreen
              draft={draft}
              status={settingsStatus}
              canSubmit={canApplyDraft}
              onDraftChange={(value) => {
                setDraft(value);
                setTested(null);
              }}
              onTest={() => void runTest()}
              onCancel={() => {
                setDraft(address);
                closeSettings();
              }}
              onSubmit={() => {
                const next = draft.trim();
                closeSettings();
                // Confirming the address already connected is no change.
                if (next !== address) void connect(next);
              }}
              onUseOwn={
                isTauri() && !isOwnAddress(address)
                  ? () => {
                      closeSettings();
                      void connect(DEFAULT_TAURI_SERVER_URL);
                    }
                  : undefined
              }
            />
          ) : (
            <>
              <h1 className={`${authScreenTitle} mb-2`}>Message Crate</h1>
              {/* A failed start says so in the card itself; the word above it
                  would only repeat "Disconnected". */}
              {localFailed && state !== "connected" ? null : (
                <ServerStatus
                  state={localStarting && state !== "connected" ? "connecting" : state}
                  label={state === "connected" ? undefined : startingLabel(localServer)}
                  className="mb-5 text-center"
                />
              )}

              {/* The card waits for the server's own answer as well as for the
                  connection: which forms belong here is the server's to say, and
                  showing a login to an unclaimed Message Crate would offer a door that
                  opens onto nothing. */}
              {localFailed && state !== "connected" ? (
                <StartFailed status={localFailed} onRetry={retryLocalServer} />
              ) : localStarting && !hasConnectedOnce ? (
                // Nothing to log in to yet. The skeleton holds the card's
                // shape for the seconds a start takes.
                <FormSkeleton />
              ) : hasConnectedOnce && serverState ? (
                <LocalAuthTabs
                  serverUrl={address}
                  serverState={serverState}
                  disabled={state !== "connected"}
                />
              ) : state === "disconnected" ? (
                // No server answered, so none has said which forms it offers.
                // Login is the one every claimed Message Crate has, and it is shown
                // disabled: a placeholder here would read as "still loading"
                // for as long as the server stays down. The way on is Change
                // server address, below.
                <LocalAuthTabs serverUrl={address} serverState="closed" disabled />
              ) : (
                <FormSkeleton />
              )}

              <OrRule />
              {/* Beside whatever the forms above offer, for as long as the
                  server says the Demo Account exists. */}
              {hasConnectedOnce && demoAccount ? (
                <ExploreDemoAccountButton serverUrl={address} disabled={state !== "connected"} />
              ) : null}
              <div className="mt-4 text-center">
                <button
                  type="button"
                  className={accentLink}
                  onClick={() => {
                    setDraft(address);
                    setTested(null);
                    setSettingsOpen(true);
                  }}
                >
                  Change server address
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
