/**
 * How the auth card is getting on with the server it resolved.
 *
 * `untested` is the settings screen's own state and never the card's: an
 * address that has been typed but not tried yet. It is a fourth answer rather
 * than a shade of the other three, because "we have not asked" is not the same
 * as reaching a server, failing to reach one, or being part-way through.
 */
export type ServerConnection = "connecting" | "connected" | "disconnected" | "untested";

const WORD: Record<ServerConnection, string> = {
  connecting: "Connecting",
  connected: "Connected",
  disconnected: "Disconnected",
  untested: "Not tested",
};

/**
 * The word carries the state on its own — there is no indicator dot. Connecting
 * keeps the slow flash the old light had, moved onto the text as an opacity
 * pulse, because scaling type wobbles the baseline underneath it.
 *
 * Untested is grey and still: green and red are answers about a server, and an
 * address nobody has tried has not earned either one. It does not pulse, since
 * nothing is happening for the pulse to stand for.
 */
const TONE: Record<ServerConnection, string> = {
  connecting: "text-text motion-safe:animate-pulse",
  connected: "text-ok",
  disconnected: "text-danger",
  untested: "text-muted",
};

export interface ServerStatusProps {
  state: ServerConnection;
  /**
   * Words to show in place of the state's own. The desktop app uses it while
   * it starts its own Message Crate: the state is still "connecting", and
   * what the person is waiting for has a better name than that.
   */
  label?: string;
  className?: string;
}

/**
 * One word naming the server connection. It replaces the old host-and-dot line:
 * the address is a setting, not something to read on every visit, so what is
 * left is the only part a person acts on. `m-0` because theme.css leaves out
 * Tailwind's preflight, so a paragraph otherwise carries the browser's own
 * margins and floats away from the line it belongs under; the gap below is the
 * caller's to set.
 */
export default function ServerStatus({ state, label, className }: ServerStatusProps) {
  return (
    <p
      role="status"
      className={`m-0 text-[0.813rem] font-medium ${TONE[state]} ${className ?? ""}`}
    >
      {label ?? WORD[state]}
    </p>
  );
}
