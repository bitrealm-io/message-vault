import { QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { ApiError, getToken, setAccountId, setBaseUrl, setToken } from "./api";
import { parsePersistedAuth } from "./authGuards";
import { createQueryClient } from "./routeQuery";
import { isUploadRunning, pauseRunningUpload } from "./runningUpload";
import { getSession, logout as serverLogout } from "./serverApi";
import { readPref, removePref, writePref } from "./storage";
import { isTauri } from "./tauri-check";
import { fetchAccountProfileFor } from "./useAccountProfile";

interface AuthState {
  serverUrl: string;
  token: string | null;
  accountId: number | null;
  isAuthenticated: boolean;
}

interface AuthContextValue extends AuthState {
  login: (serverUrl: string, token: string, accountId: number) => Promise<void>;
  /** Save a new session token after the user changes their password. */
  updateToken: (token: string) => void;
  /**
   * Revoke the server session (best-effort) and clear the saved login.
   *
   * An Upload that is running is paused first, and the session is revoked
   * only once the pause is recorded: the push sends this session's token.
   * Unless `ask` is false, logout first asks whether to pause it, and does
   * nothing when the person goes back.
   */
  logout: (options?: { ask?: boolean }) => Promise<void>;
  setServer: (url: string) => void;
  /**
   * Check the saved login again, after a startup check the server never
   * answered. `serverUrl` is the address just found reachable: a login saved
   * for any other address is left alone, so a token only ever goes to the
   * server that issued it. Does nothing when no login is saved.
   */
  retrySavedLogin: (serverUrl: string) => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

const STORAGE_KEY = "message-crate-auth";

const UPLOAD_RUNNING_PROMPT =
  "An Upload is running. Logging out pauses it; you can resume it after you log in.";

/** Max time to wait for the server logout request before clearing local state. */
const LOGOUT_TIMEOUT_MS = 2000;

/** AbortSignal that fires after {@link LOGOUT_TIMEOUT_MS}. */
function logoutTimeoutSignal(): AbortSignal {
  if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
    return AbortSignal.timeout(LOGOUT_TIMEOUT_MS);
  }
  const controller = new AbortController();
  setTimeout(() => controller.abort(), LOGOUT_TIMEOUT_MS);
  return controller.signal;
}

/** Read the last saved login from browser storage. */
function loadPersisted(): { serverUrl: string; token: string; accountId: number } | null {
  const raw = readPref(STORAGE_KEY);
  if (!raw) return null;
  const parsed = parsePersistedAuth(raw);
  if (!parsed) return null;
  return {
    serverUrl: parsed.serverUrl,
    token: parsed.token,
    accountId: parsed.accountId,
  };
}

/** Write the current login to browser storage. Passwords are never stored. */
function persistState(state: AuthState) {
  writePref(
    STORAGE_KEY,
    JSON.stringify({
      serverUrl: state.serverUrl,
      token: state.token,
      accountId: state.accountId,
    }),
  );
}

/** Remove the saved login from browser storage. */
function clearPersisted() {
  removePref(STORAGE_KEY);
}

/**
 * Holds login state for the app and restores a saved session on startup.
 *
 * It also builds the query client every screen fetches through. A query or
 * mutation the server refuses because the session has ended has to log the
 * person out, and the client only learns how at construction, so the provider
 * that owns the logout owns the client too.
 */
export function AuthProvider({ children }: { children: ReactNode }) {
  const sessionEnded = useRef<() => void>(() => {});
  const [queryClient] = useState(() =>
    createQueryClient({ onUnauthorized: () => sessionEnded.current() }),
  );
  return (
    <QueryClientProvider client={queryClient}>
      <SessionProvider sessionEnded={sessionEnded}>{children}</SessionProvider>
    </QueryClientProvider>
  );
}

function SessionProvider({
  children,
  sessionEnded,
}: {
  children: ReactNode;
  /** Set here to what the query client calls when the server ends the session. */
  sessionEnded: { current: () => void };
}) {
  // Talking to the client directly rather than through `routeQuery`'s hooks,
  // which read `useAuth` from this provider.
  const queryClient = useQueryClient();
  const resetRouteCache = useCallback(() => {
    queryClient.clear();
  }, [queryClient]);
  const [restored, setRestored] = useState(false);
  // Incremented on login and logout so an older profile request is ignored.
  const authEpoch = useRef(0);
  const [state, setState] = useState<AuthState>(() => {
    const persisted = loadPersisted();
    // An empty server URL is allowed: it means "same host as this page".
    if (persisted?.token && typeof persisted.serverUrl === "string") {
      // Apply before children mount. Otherwise Contact Groups loads without
      // a token, fails, and the sidebar stays on "No group" only.
      setBaseUrl(persisted.serverUrl);
      setToken(persisted.token);
      setAccountId(persisted.accountId);
      return {
        serverUrl: persisted.serverUrl,
        token: persisted.token,
        accountId: persisted.accountId,
        isAuthenticated: true,
      };
    }
    return {
      serverUrl: typeof persisted?.serverUrl === "string" ? persisted.serverUrl : "",
      token: null,
      accountId: null,
      isAuthenticated: false,
    };
  });

  useEffect(() => {
    setBaseUrl(state.serverUrl);
    setToken(state.token);
    setAccountId(state.accountId);
  }, [state.serverUrl, state.token, state.accountId]);

  // Check that the restored token still works.
  useEffect(() => {
    if (!state.isAuthenticated || restored) return;

    let cancelled = false;
    const validate = async () => {
      try {
        setBaseUrl(state.serverUrl);
        setToken(state.token);
        setAccountId(state.accountId);
        await getSession();
        if (cancelled) return;

        // Warm the profile before the app renders. Whether this account still
        // owes profile setup is read from it, so fetching it here keeps the
        // guards from deciding against an empty cache.
        try {
          await fetchAccountProfileFor(queryClient, state.accountId, true);
        } catch {
          // The guards treat a profile that has not loaded as "not decided
          // yet" and hold, so there is nothing to fall back to here.
        }

        if (!cancelled) setRestored(true);
      } catch (err) {
        // Only the server can say a token is no longer valid, and it says so
        // with a 401. A request nothing answered, or a 502 from a proxy, says
        // nothing about the token, so the saved login stays for
        // `retrySavedLogin`. Either way this session shows the login screen.
        if (!cancelled) {
          authEpoch.current++;
          setToken(null);
          setAccountId(null);
          if (err instanceof ApiError && err.status === 401) clearPersisted();
          setState((s) => ({
            ...s,
            token: null,
            accountId: null,
            isAuthenticated: false,
          }));
          setRestored(true);
        }
      }
    };
    validate();
    return () => {
      cancelled = true;
    };
  }, [state.isAuthenticated, restored, state.serverUrl, state.token, queryClient, state.accountId]);

  const setServer = useCallback((url: string) => {
    setBaseUrl(url);
    setState((s) => ({ ...s, serverUrl: url }));
  }, []);

  const retrySavedLogin = useCallback((serverUrl: string) => {
    const persisted = loadPersisted();
    if (!persisted?.token || persisted.serverUrl !== serverUrl) return;
    // Back to the state the app starts in, which runs the check above again.
    setState({
      serverUrl: persisted.serverUrl,
      token: persisted.token,
      accountId: persisted.accountId,
      isAuthenticated: true,
    });
    setRestored(false);
  }, []);

  const login = useCallback(
    async (serverUrl: string, token: string, accountId: number) => {
      const epoch = ++authEpoch.current;
      // One call, and it cannot be incomplete: every cached entry is named
      // with the account that filled it, so this only releases memory.
      resetRouteCache();
      setBaseUrl(serverUrl);
      setToken(token);
      // The profile is the account's own row, addressed by this id, so the
      // client must know it before the fetch below.
      setAccountId(accountId);

      // Fetch the profile before the app renders: it carries whether this
      // account still owes profile setup or a password change, and the guards
      // read it from there rather than from anything decided here.
      await fetchAccountProfileFor(queryClient, accountId, true);

      if (authEpoch.current !== epoch) return; // A later login or logout replaced this one.

      const newState: AuthState = {
        serverUrl,
        token,
        accountId,
        isAuthenticated: true,
      };
      persistState(newState);
      setState(newState);
      setRestored(true);
    },
    [
      // One call, and it cannot be incomplete: every cached entry is named
      // with the account that filled it, so this only releases memory.
      resetRouteCache,
      queryClient,
    ],
  );

  const updateToken = useCallback((token: string) => {
    setToken(token);
    setState((s) => {
      if (!s.isAuthenticated) return s;
      const next: AuthState = { ...s, token };
      persistState(next);
      return next;
    });
  }, []);

  /** Forget the login on this side: token, cached data, and the saved login. */
  const clearSession = useCallback(() => {
    authEpoch.current++;
    setToken(null);
    setAccountId(null);
    resetRouteCache();
    clearPersisted();
    setState((s) => ({
      ...s,
      token: null,
      accountId: null,
      isAuthenticated: false,
    }));
  }, [resetRouteCache]);

  /** Tell the server to end the session, then forget it here. */
  const revokeSession = useCallback(async () => {
    authEpoch.current++;
    // Tell the server to end the session while the token is still set on the API client.
    // Await so close-to-quit can finish (or time out) before the WebView dies.
    if (getToken()) {
      try {
        await serverLogout({ signal: logoutTimeoutSignal() });
      } catch {
        // Server unreachable, 401, or timeout — still clear the local session.
      }
    }
    clearSession();
  }, [clearSession]);

  // The question logout asks while an Upload runs: open, and then pausing
  // once the person chose to log out. `answer` settles the logout waiting on it.
  const [uploadPrompt, setUploadPrompt] = useState<"asking" | "pausing" | null>(null);
  const answer = useRef<(logOut: boolean) => void>(() => {});

  const logout = useCallback(
    async ({ ask = true }: { ask?: boolean } = {}) => {
      if (ask && isUploadRunning()) {
        const logOut = await new Promise<boolean>((resolve) => {
          answer.current = resolve;
          setUploadPrompt("asking");
        });
        if (!logOut) return;
      }
      try {
        await pauseRunningUpload();
        await revokeSession();
      } finally {
        setUploadPrompt(null);
      }
    },
    [revokeSession],
  );

  // The server has refused the token, so the session is already over there
  // and there is nothing to tell it. With no token set, the 401 came from a
  // request made before login, and there is no session here to end either.
  // An Upload that is running is paused all the same: its push sends the
  // same token, so it would only record every remaining conversation as
  // failed, and the run stays resumable.
  useEffect(() => {
    sessionEnded.current = () => {
      if (!getToken()) return;
      void pauseRunningUpload();
      clearSession();
    };
  }, [sessionEnded, clearSession]);

  // Desktop only: on window close, revoke the session then quit.
  const closingRef = useRef(false);
  useEffect(() => {
    if (!isTauri()) return;

    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      try {
        const win = getCurrentWindow();
        unlisten = await win.onCloseRequested(async (event) => {
          event.preventDefault();
          if (closingRef.current) return;
          closingRef.current = true;
          try {
            // No pause: closing the window ends the push with the app, and
            // the run resumes from what the push journal recorded as sent.
            await revokeSession();
            await win.destroy();
          } catch {
            // Destroy failed or window already gone — allow another close attempt.
            closingRef.current = false;
          }
        });
        if (cancelled) {
          unlisten();
          unlisten = undefined;
        }
      } catch {
        // Missing window permissions or not a real Tauri window — leave close alone.
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [revokeSession]);

  return (
    <AuthContext.Provider
      value={{ ...state, login, logout, updateToken, setServer, retrySavedLogin }}
    >
      {children}
      <ConfirmDialog
        open={uploadPrompt !== null}
        title="Log out"
        body={UPLOAD_RUNNING_PROMPT}
        confirmLabel="Log out"
        cancelLabel="Go back"
        busy={uploadPrompt === "pausing"}
        busyLabel="Pausing…"
        onConfirm={() => {
          setUploadPrompt("pausing");
          answer.current(true);
        }}
        onClose={() => {
          setUploadPrompt(null);
          answer.current(false);
        }}
      />
    </AuthContext.Provider>
  );
}

/** Current login state. Must be called under AuthProvider. */
export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
