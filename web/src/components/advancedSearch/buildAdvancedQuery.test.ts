import { describe, expect, it } from "vitest";
import {
  buildContactsQuery,
  buildMessagesQuery,
  buildTrashQuery,
  type ContactsQueryInput,
  canSubmitContacts,
  canSubmitMessages,
  composeCountComparison,
  EMPTY_COUNT,
  EMPTY_DATE_BOUND,
} from "./buildAdvancedQuery.ts";

/** Every Contacts field left blank, so a test can fill in only what it is about. */
const emptyContacts: ContactsQueryInput = {
  contactName: "",
  handle: "",
  firstHeardBound: EMPTY_DATE_BOUND,
  lastHeardBound: EMPTY_DATE_BOUND,
  activity: "any",
  noPreferredName: false,
  noHandle: false,
  services: [],
};

describe("composeCountComparison", () => {
  it("returns null for any or non-numeric", () => {
    expect(composeCountComparison({ comparator: "any", value: "3" })).toBeNull();
    expect(composeCountComparison({ comparator: "=", value: "" })).toBeNull();
    expect(composeCountComparison({ comparator: ">", value: "x" })).toBeNull();
  });

  it("formats comparator and digits", () => {
    expect(composeCountComparison({ comparator: "=", value: "2" })).toBe("=2");
    expect(composeCountComparison({ comparator: "<", value: " 10 " })).toBe("<10");
  });
});

describe("buildMessagesQuery", () => {
  it("returns empty for blank fields", () => {
    expect(
      buildMessagesQuery({
        nameOrHandle: "",
        handle: "",
        msgType: "all",
        participants: EMPTY_COUNT,
      }),
    ).toBe("");
    expect(
      canSubmitMessages({
        nameOrHandle: "",
        handle: "",
        msgType: "all",
        participants: EMPTY_COUNT,
      }),
    ).toBe(false);
  });

  it("assembles name, handle, type, and participants tokens", () => {
    expect(
      buildMessagesQuery({
        nameOrHandle: "jane",
        handle: "",
        msgType: "direct",
        participants: EMPTY_COUNT,
      }),
    ).toBe("jane kind:direct");
    expect(
      buildMessagesQuery({
        nameOrHandle: "",
        handle: "+1555",
        msgType: "group",
        participants: { comparator: ">", value: "3" },
      }),
    ).toBe("handle:+1555 kind:group participants:>3");
  });
});

describe("buildContactsQuery", () => {
  it("returns empty when nothing is filled in", () => {
    expect(buildContactsQuery(emptyContacts)).toBe("");
    expect(canSubmitContacts(emptyContacts)).toBe(false);
  });

  it("assembles contact tokens", () => {
    expect(
      buildContactsQuery({
        contactName: "ana",
        handle: "+1 555",
        firstHeardBound: { op: "after", start: "2019-01-01", end: "" },
        lastHeardBound: { op: "between", start: "2022-01-01", end: "2023-01-01" },
        activity: "messages",
        noPreferredName: true,
        noHandle: false,
        services: ["whatsapp"],
      }),
    ).toBe(
      'ana handle:"+1 555" first-heard:>=2019-01-01 last-heard:2022-01-01..2023-01-01 messages:>0 name:none service:whatsapp',
    );
  });

  it("leaves the heard dates out of a Trash search, which Conversations also runs", () => {
    expect(
      buildTrashQuery({
        ...emptyContacts,
        contactName: "ana",
        firstHeardBound: { op: "after", start: "2019-01-01", end: "" },
        lastHeardBound: { op: "before", start: "2022-01-01", end: "" },
      }),
    ).toBe("ana");
  });

  it("asks for contacts with no messages and no identity", () => {
    expect(buildContactsQuery({ ...emptyContacts, activity: "no-messages", noHandle: true })).toBe(
      "messages:0 handle:none",
    );
  });

  it("puts several ticked transports in one word, so any of them matches", () => {
    expect(buildContactsQuery({ ...emptyContacts, services: ["whatsapp"] })).toBe(
      "service:whatsapp",
    );
    expect(buildContactsQuery({ ...emptyContacts, services: ["sms", "mms", "rcs"] })).toBe(
      "service:sms,mms,rcs",
    );
  });
});
