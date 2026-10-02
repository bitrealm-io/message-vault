import type { ServerConnection } from "./ServerStatus";

/**
 * Which Message Crate the login card is on, and which one it is trying.
 *
 * They are two values because they are two things. The card stays on the
 * address it has until another one answers: trying an address that does not
 * answer must neither move the card to it nor quietly put the card back where
 * it was, and both happened while the card kept one address for the two
 * (#1235).
 */
export interface Connection {
  /** The address the card is on: the last one that answered, or the saved one until one has. */
  address: string;
  /** How `address` answered when it was last asked. */
  status: "connecting" | "connected" | "disconnected";
  /** An address being asked now, or null. It may be `address` itself. */
  trying: string | null;
  /**
   * The last address tried in place of `address` that did not answer, with
   * the reason when there is one beyond silence. Cleared by the next try.
   */
  failed: { address: string; message: string | null } | null;
  /** Whether any address has answered since the card opened. */
  hasConnectedOnce: boolean;
}

export type ConnectionEvent =
  | { type: "try"; address: string }
  | { type: "answered"; address: string }
  | { type: "noAnswer"; address: string; message?: string };

/** The card as it opens: on the saved address, which is asked at once. */
export function initialConnection(address: string): Connection {
  return { address, status: "connecting", trying: null, failed: null, hasConnectedOnce: false };
}

export function connectionReducer(state: Connection, event: ConnectionEvent): Connection {
  switch (event.type) {
    case "try":
      return {
        ...state,
        trying: event.address,
        status: event.address === state.address ? "connecting" : state.status,
        failed: null,
      };
    case "answered":
      // An answer for an address no longer being tried describes a server the
      // card has moved on from.
      if (state.trying !== event.address) return state;
      return {
        address: event.address,
        status: "connected",
        trying: null,
        failed: null,
        hasConnectedOnce: true,
      };
    case "noAnswer":
      if (state.trying !== event.address) return state;
      if (event.address === state.address) {
        return { ...state, trying: null, status: "disconnected" };
      }
      return {
        ...state,
        trying: null,
        failed: { address: event.address, message: event.message ?? null },
      };
  }
}

/** The one word the card shows: connecting while any address is being asked. */
export function shownState(state: Connection): ServerConnection {
  return state.trying !== null ? "connecting" : state.status;
}
