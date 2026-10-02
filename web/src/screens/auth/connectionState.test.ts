import { describe, expect, it } from "vitest";
import {
  type Connection,
  connectionReducer,
  initialConnection,
  shownState,
} from "./connectionState";

const A = "http://crate-a.example:8080";
const B = "http://crate-b.example:8080";

/** Connected to A, as the card is once A has answered. */
function connectedToA(): Connection {
  let c = initialConnection(A);
  c = connectionReducer(c, { type: "try", address: A });
  return connectionReducer(c, { type: "answered", address: A });
}

describe("connectionReducer", () => {
  it("starts on the saved address, connecting", () => {
    const c = initialConnection(A);
    expect(c.address).toBe(A);
    expect(shownState(c)).toBe("connecting");
    expect(c.failed).toBeNull();
  });

  it("moves to an address only once it answers", () => {
    let c = connectedToA();
    c = connectionReducer(c, { type: "try", address: B });
    expect(c.address).toBe(A);
    expect(c.trying).toBe(B);
    expect(shownState(c)).toBe("connecting");

    c = connectionReducer(c, { type: "answered", address: B });
    expect(c.address).toBe(B);
    expect(c.trying).toBeNull();
    expect(shownState(c)).toBe("connected");
  });

  it("stays connected to A and names B when B does not answer", () => {
    let c = connectedToA();
    c = connectionReducer(c, { type: "try", address: B });
    c = connectionReducer(c, { type: "noAnswer", address: B });

    expect(c.address).toBe(A);
    expect(c.trying).toBeNull();
    expect(shownState(c)).toBe("connected");
    expect(c.failed).toEqual({ address: B, message: null });
  });

  it("keeps the reason a try failed, when there is one", () => {
    let c = connectedToA();
    c = connectionReducer(c, { type: "try", address: B });
    c = connectionReducer(c, { type: "noAnswer", address: B, message: "Port taken." });
    expect(c.failed).toEqual({ address: B, message: "Port taken." });
  });

  it("is disconnected, with nothing else to report, when its own address does not answer", () => {
    let c = initialConnection(A);
    c = connectionReducer(c, { type: "try", address: A });
    c = connectionReducer(c, { type: "noAnswer", address: A });
    expect(shownState(c)).toBe("disconnected");
    expect(c.failed).toBeNull();
  });

  it("ignores an answer for an address it is no longer trying", () => {
    let c = initialConnection(A);
    c = connectionReducer(c, { type: "try", address: A });
    c = connectionReducer(c, { type: "try", address: B });
    const late = connectionReducer(c, { type: "answered", address: A });
    expect(late).toBe(c);
    const lateFailure = connectionReducer(c, { type: "noAnswer", address: A });
    expect(lateFailure).toBe(c);
  });

  it("clears the last failure when another address is tried", () => {
    let c = connectedToA();
    c = connectionReducer(c, { type: "try", address: B });
    c = connectionReducer(c, { type: "noAnswer", address: B });
    c = connectionReducer(c, { type: "try", address: B });
    expect(c.failed).toBeNull();
  });

  it("remembers that a connection was made, through later failures", () => {
    let c = initialConnection(A);
    expect(c.hasConnectedOnce).toBe(false);
    c = connectedToA();
    c = connectionReducer(c, { type: "try", address: A });
    c = connectionReducer(c, { type: "noAnswer", address: A });
    expect(c.hasConnectedOnce).toBe(true);
  });
});
