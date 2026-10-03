/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render as rtlRender, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ContactDetail } from "../lib/contactDetail";
import { keys } from "../lib/queryKeys";
import { routeQueryKey } from "../lib/routeQueryKey";
import { mockedAuth, renderWithProviders as render, testQueryClient } from "../test/providers";
import CheckedContactsPanel from "./CheckedContactsPanel";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));
const summaries = vi.fn();

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  getContactSummaries: (...args: unknown[]) => summaries(...args),
}));

beforeEach(() => {
  summaries.mockReset();
  summaries.mockResolvedValue({ items: [] });
});

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

  // The app's own :focus-visible outline loses to the outline-none utility, so
  // a sortable header draws the style guide's ring itself.
  it("draws the focus ring on every sortable column header", () => {
    render(<CheckedContactsPanel contacts={[{ id: "1", name: "Ada" }]} onClear={() => {}} />);
    const sortable = screen
      .getAllByRole("columnheader")
      .filter((header) => header.hasAttribute("aria-sort"));
    expect(sortable).toHaveLength(6);
    for (const header of sortable) {
      expect(header.className).toContain("focus-visible:ring-2 focus-visible:ring-accent");
    }
  });

  it("draws the focus ring on every contact's row", () => {
    render(
      <CheckedContactsPanel
        contacts={[
          { id: "1", name: "Ada" },
          { id: "2", name: "Bob" },
        ]}
        onClear={() => {}}
      />,
    );
    const bodyRows = screen.getAllByRole("row").slice(1);
    expect(bodyRows).toHaveLength(2);
    for (const row of bodyRows) {
      expect(row.className).toContain(
        "focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent",
      );
    }
  });

  it("says the figures could not be loaded and loads them on Try again", async () => {
    summaries.mockRejectedValueOnce(new Error("The server could not answer."));
    summaries.mockResolvedValueOnce({
      items: [
        {
          id: 1,
          name: "Ada",
          individual_conversations: 2,
          group_conversations: 1,
          individual_message_count: 314,
          group_message_count: 15,
        },
      ],
    });
    const user = userEvent.setup();

    render(<CheckedContactsPanel contacts={[{ id: "1", name: "Ada" }]} onClear={() => {}} />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The figures for these contacts could not be loaded.");
    expect(alert.textContent).toContain("The server could not answer.");

    await user.click(screen.getByRole("button", { name: "Try again" }));

    await waitFor(() => {
      expect(screen.getByText("314")).toBeTruthy();
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("counts a conversation two of a cached contact's identities share once", async () => {
    // A phone and an email take part in one group conversation, the contact's
    // only one. While the summaries load, the row shows the cached contact,
    // and that must say one conversation, as the summaries will (#1248).
    summaries.mockReturnValue(new Promise(() => {}));
    const shared = {
      start_date: "2024-06-01T12:00:00Z",
      end_date: "2024-06-01T12:00:00Z",
      conversations: 1,
      direct_messages: 0,
      group_messages: 2,
    };
    const sam: ContactDetail = {
      id: 1,
      name: "Sam",
      unknown: false,
      last_modified: "2024-01-01T00:00:00Z",
      identities: [
        { ...shared, address: "+15550001", service: "phone" },
        { ...shared, address: "sam@example.com", service: "email" },
      ],
      direct_conversations: 0,
      group_conversations: 1,
      total_messages: 4,
      groups: [],
    };
    const client = testQueryClient({ keepUnread: true });
    client.setQueryData(routeQueryKey(7, keys.contacts.detail(1)), sam);

    rtlRender(<CheckedContactsPanel contacts={[{ id: "1", name: "Sam" }]} onClear={() => {}} />, {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });

    const row = screen.getByRole("rowheader", { name: "Sam" }).closest("[role=row]");
    const cells = within(row as HTMLElement)
      .getAllByRole("gridcell")
      .map((c) => c.textContent);
    expect(cells).toEqual(["2024-06-01", "2024-06-01", "1", "0", "4"]);
  });
});
