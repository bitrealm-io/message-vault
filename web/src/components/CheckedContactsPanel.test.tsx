/** @vitest-environment jsdom */

import { cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, renderWithProviders as render } from "../test/providers";
import CheckedContactsPanel from "./CheckedContactsPanel";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));
vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  getContactSummaries: () => Promise.resolve({ items: [] }),
}));

afterEach(cleanup);

describe("CheckedContactsPanel", () => {
  it("names its columns the way the contact's identity table does", () => {
    render(
      <CheckedContactsPanel
        contacts={[
          { id: "1", name: "Ada" },
          { id: "2", name: "Bob" },
        ]}
        onClear={() => {}}
      />,
    );

    const headers = screen
      .getAllByRole("columnheader")
      .map((h) => h.textContent?.replace(/[▲▼]/g, "").trim());
    expect(headers).toEqual([
      "Contact",
      "First heard from",
      "Last heard from",
      "Conversations",
      "DirectMessages",
      "GroupMessages",
    ]);
  });
});
