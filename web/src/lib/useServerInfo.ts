import { keys } from "./queryKeys";
import { useRouteQuery } from "./routeQuery";
import { getServerState } from "./serverApi";

/**
 * What the server says about itself to a logged-in screen: its Build and its
 * Schema Fingerprint. The same `GET /v1/server` the entry screen reads for the
 * server's state (`useServerState`), asked again here because that query is
 * keyed by address and never goes stale, and a server's Build changes under an
 * open tab when the server is upgraded.
 */
export function useServerInfo() {
  return useRouteQuery(keys.serverInfo.all, (signal) => getServerState({ signal }));
}
