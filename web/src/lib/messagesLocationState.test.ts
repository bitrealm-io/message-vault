import { describe, expect, it } from "vitest";
import { asMessagesLocationState } from "./messagesLocationState.ts";

const conversation = {
  id: 42,
  participants: [],
  message_count: 1,
  last_message_at: "2026-01-01T00:00:00Z",
  date_range_start: null,
  date_range_end: null,
  service: "imessage",
  is_group: false,
  label: null,
};

const preview = {
  id: "c1",
  name: "Ada",
  addresses: ["+15550001"],
  handleCount: 1,
};

describe("asMessagesLocationState", () => {
  it("accepts conversation and openContactId", () => {
    expect(
      asMessagesLocationState({
        conversation,
        openContactId: "c1",
      }),
    ).toEqual({ conversation, openContactId: "c1" });
  });

  it("accepts openContactId alone", () => {
    expect(asMessagesLocationState({ openContactId: "c1" })).toEqual({
      openContactId: "c1",
    });
  });

  it("rejects non-objects and invalid shapes", () => {
    expect(asMessagesLocationState(null)).toBeNull();
    expect(asMessagesLocationState("x")).toBeNull();
    expect(asMessagesLocationState({ conversation: { id: -1 } })).toBeNull();
    expect(asMessagesLocationState({ openContactId: 5 })).toBeNull();
    expect(asMessagesLocationState({})).toBeNull();
  });

  it("rejects a conversation whose id is not a positive integer", () => {
    expect(asMessagesLocationState({ conversation: { ...conversation, id: "42" } })).toBeNull();
    expect(asMessagesLocationState({ conversation: { ...conversation, id: 0 } })).toBeNull();
  });

  it("accepts openContactPreview when it matches openContactId", () => {
    expect(
      asMessagesLocationState({
        conversation,
        openContactId: "c1",
        openContactPreview: preview,
      }),
    ).toEqual({ conversation, openContactId: "c1", openContactPreview: preview });
  });

  it("drops openContactPreview when id does not match openContactId", () => {
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { ...preview, id: "other" },
      }),
    ).toEqual({ openContactId: "c1" });
  });

  it("drops malformed openContactPreview without rejecting the rest of state", () => {
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { id: "c1" },
      }),
    ).toEqual({ openContactId: "c1" });
  });

  it("drops openContactPreview when name is empty", () => {
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { ...preview, name: "" },
      }),
    ).toEqual({ openContactId: "c1" });
  });

  it("drops openContactPreview when handleCount is negative or not an integer", () => {
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { ...preview, handleCount: -1 },
      }),
    ).toEqual({ openContactId: "c1" });
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { ...preview, handleCount: 1.5 },
      }),
    ).toEqual({ openContactId: "c1" });
    expect(
      asMessagesLocationState({
        openContactId: "c1",
        openContactPreview: { ...preview, handleCount: 501 },
      }),
    ).toEqual({ openContactId: "c1" });
  });
});
