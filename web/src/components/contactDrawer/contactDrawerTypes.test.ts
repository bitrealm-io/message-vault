import { describe, expect, it } from "vitest";
import {
  contactPreviewFromListRow,
  contactPreviewFromThreadParticipants,
  HANDLE_STUB_PLACEHOLDER,
  previewHandleStubRows,
  sameContactPreviews,
} from "./contactDrawerTypes";

describe("contactPreviewFromListRow", () => {
  it("maps list-API identity_count onto preview handleCount", () => {
    expect(
      contactPreviewFromListRow({
        id: "1",
        name: "Ada",
        addresses: ["+15550001", "15550001"],
        identity_count: 1,
        groups: ["Family"],
      }),
    ).toEqual({
      id: "1",
      name: "Ada",
      addresses: ["+15550001", "15550001"],
      handleCount: 1,
      groups: ["Family"],
    });
  });
});

describe("previewHandleStubRows", () => {
  it("stubs one row when preview lists raw and normalized forms of one identity", () => {
    const rows = previewHandleStubRows(["+1555000b", "1555000b"], 1);
    expect(rows.map((r) => r.address)).toEqual(["+1555000b"]);
  });

  it("uses unique identities when handleCount is missing", () => {
    const rows = previewHandleStubRows(["+1555000b", "1555000b"], undefined);
    expect(rows.map((r) => r.address)).toEqual(["+1555000b"]);
  });

  it("pads with a placeholder when handleCount is larger than the unique list", () => {
    const rows = previewHandleStubRows(["+1555000b"], 3);
    expect(rows.map((r) => r.address)).toEqual([
      "+1555000b",
      HANDLE_STUB_PLACEHOLDER,
      HANDLE_STUB_PLACEHOLDER,
    ]);
  });

  it("returns no rows when handleCount is 0", () => {
    expect(previewHandleStubRows(["+1555000b"], 0)).toEqual([]);
  });

  it("keeps two distinct phones when handleCount is 2", () => {
    const rows = previewHandleStubRows(["+15550001", "15550001", "+15550002", "15550002"], 2);
    expect(rows.map((r) => r.address)).toEqual(["+15550001", "+15550002"]);
  });
});

describe("contactPreviewFromThreadParticipants", () => {
  it("builds a preview from matching conversation participants", () => {
    expect(
      contactPreviewFromThreadParticipants("1", [
        { contact_id: 1, handle: "+15550001", name: "Ada" },
        { contact_id: 2, handle: "+15550002", name: "Bob" },
      ]),
    ).toEqual({
      id: "1",
      name: "Ada",
      addresses: ["+15550001"],
      handleCount: 1,
    });
  });

  it("counts two distinct phones for the same contact as two identities", () => {
    expect(
      contactPreviewFromThreadParticipants("1", [
        { contact_id: 1, handle: "+15550001", name: "Ada" },
        { contact_id: 1, handle: "+15550002", name: "Ada" },
      ]),
    ).toMatchObject({
      addresses: ["+15550001", "+15550002"],
      handleCount: 2,
    });
  });

  it("collapses raw and normalized forms of the same phone to handleCount 1", () => {
    expect(
      contactPreviewFromThreadParticipants("1", [
        { contact_id: 1, handle: "+15550001", name: "Ada" },
        { contact_id: 1, handle: "15550001", name: "Ada" },
      ])?.handleCount,
    ).toBe(1);
  });

  it("falls back to the handle when no display name is set", () => {
    expect(
      contactPreviewFromThreadParticipants("1", [{ contact_id: 1, handle: "+15550001", name: "" }])
        ?.name,
    ).toBe("+15550001");
  });

  it("stubs at least one identity when matched addresses are empty", () => {
    expect(
      contactPreviewFromThreadParticipants("1", [{ contact_id: 1, handle: "", name: "Ada" }]),
    ).toMatchObject({
      name: "Ada",
      addresses: [],
      handleCount: 1,
    });
  });

  it("returns null when no participant matches the contact id", () => {
    expect(
      contactPreviewFromThreadParticipants("missing", [
        { contact_id: 1, handle: "+15550001", name: "Ada" },
      ]),
    ).toBeNull();
  });
});

describe("sameContactPreviews", () => {
  const ada = {
    id: "1",
    name: "Ada",
    addresses: ["+15550001"],
    handleCount: 1,
    groups: ["Family"],
  };

  it("treats a re-mapped but equal list as unchanged", () => {
    expect(
      sameContactPreviews([ada], [{ ...ada, addresses: ["+15550001"], groups: ["Family"] }]),
    ).toBe(true);
  });

  it("reports a changed name", () => {
    expect(sameContactPreviews([ada], [{ ...ada, name: "Grace" }])).toBe(false);
  });

  it("reports changed group membership", () => {
    expect(sameContactPreviews([ada], [{ ...ada, groups: ["Work"] }])).toBe(false);
  });

  it("reports a changed length", () => {
    expect(sameContactPreviews([ada], [])).toBe(false);
    expect(sameContactPreviews([], [ada])).toBe(false);
  });

  it("treats two empty lists as unchanged", () => {
    expect(sameContactPreviews([], [])).toBe(true);
  });

  it("distinguishes a missing addresses list from an empty one", () => {
    expect(
      sameContactPreviews([{ id: "1", name: "Ada" }], [{ id: "1", name: "Ada", addresses: [] }]),
    ).toBe(false);
  });
});
