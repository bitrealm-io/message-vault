/** @vitest-environment jsdom */

import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, renderWithProviders as render } from "../test/providers";
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
});
