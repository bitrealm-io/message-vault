import { describe, expect, it } from "vitest";
import { contactBrowseQuery } from "./contactBrowseQuery";

describe("contactBrowseQuery", () => {
  it("builds a bare handle term when the handle needs no quoting", () => {
    expect(contactBrowseQuery("42", "all", "ann@example.com")).toBe("handle:ann@example.com");
  });

  it("quotes a handle with a space", () => {
    expect(contactBrowseQuery("42", "all", "Ann Lee")).toBe('handle:"Ann Lee"');
  });

  it("quotes a handle with parentheses, since the language reads them as grouping", () => {
    expect(contactBrowseQuery("42", "all", "Ann (Lee)")).toBe('handle:"Ann (Lee)"');
  });

  it("falls back to the contact id when there is no handle", () => {
    expect(contactBrowseQuery("42", "all")).toBe("with:#42");
    expect(contactBrowseQuery("42", "all", "")).toBe("with:#42");
    expect(contactBrowseQuery("42", "all", "   ")).toBe("with:#42");
  });

  it("narrows to kind:direct or kind:group, and leaves 'all' as the base query", () => {
    expect(contactBrowseQuery("42", "direct")).toBe("kind:direct (with:#42)");
    expect(contactBrowseQuery("42", "group")).toBe("kind:group (with:#42)");
    expect(contactBrowseQuery("42", "all")).toBe("with:#42");
    expect(contactBrowseQuery("42", "direct", "Ann Lee")).toBe('kind:direct (handle:"Ann Lee")');
  });
});
