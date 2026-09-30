import { keys } from "./queryKeys";
import { useRouteQuery } from "./routeQuery";
import { getServerState } from "./serverApi";

/**
 * What the vault says about itself to a logged-in screen: its Build and its
 * Schema Fingerprint. The same `GET /v1/server` the entry screen reads for the
 * vault's state (`useServerState`), asked again here because that query is
 * keyed by address and never goes stale, and a vault's Build changes under an
 * open tab when the vault is upgraded.
 */
export function useServerInfo() {
  return useRouteQuery(keys.serverInfo.all, (signal) => getServerState({ signal }));
}
