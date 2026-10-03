/** @vitest-environment jsdom */

import { cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../test/providers";
import { StorageSection } from "../StorageSection";

const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());
const getAccountImport = vi.hoisted(() => vi.fn());

vi.mock("../../../lib/auth", () => ({
  useAuth: () => ({ accountId: 7 }),
}));

vi.mock("../../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  listAccountImports: (...a: unknown[]) => listAccountImports(...a),
  listAccountExports: (...a: unknown[]) => listAccountExports(...a),
  getAccountImport: (...a: unknown[]) => getAccountImport(...a),
}));

/** One Import Run, told apart on screen by its source. */
function anImport(id: number) {
  return {
    id,
    source: `source-${id}`,
    status: "completed",
    started_at: "2024-01-01T00:00:00Z",
    finished_at: "2024-01-01T00:05:00Z",
    message_count: 10,
    attachment_count: 1,
    bytes_uploaded: 1024,
  };
}

describe("Import history", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getAccountProfile.mockResolvedValue({ account_id: 7, message_count: 0 });
    getAccountStorage.mockResolvedValue({
      total_bytes: 0,
      attachment_count: 0,
      conversation_count: 0,
      contact_count: 0,
      top_attachments: [],
    });
    listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    // 120 runs make three pages of 50. The server answers the page it is asked for.
    listAccountImports.mockImplementation(({ offset }: { offset: number }) =>
      Promise.resolve({ items: [anImport(offset + 1)], total: 120, limit: 50, offset }),
    );
  });

  afterEach(() => {
    cleanup();
  });

  it("asks the server for each page of 50 and stops at the last", async () => {
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<StorageSection />);

    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    const back = () => section.getByRole("button", { name: "Back" });
    const next = () => section.getByRole("button", { name: "Next" });

    expect(await section.findByText("Page 1 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 0 },
      expect.anything(),
      undefined,
    );
    expect(back()).toBeDisabled();
    expect(next()).toBeEnabled();

    await user.click(next());
    expect(await section.findByText("Page 2 of 3")).toBeInTheDocument();
    expect(listAccountImports).toHaveBeenCalledWith(
      { limit: 50, offset: 50 },
      expect.anything(),
      undefined,
    );
    expect(await section.findByText("source-51")).toBeInTheDocument();
    expect(back()).toBeEnabled();

    await user.click(next());
    expect(await section.findByText("Page 3 of 3")).toBeInTheDocument();
    await waitFor(() => expect(next()).toBeDisabled());
  });

  it("says a run that has not finished has not finished, in place of a finish time", async () => {
    const running = {
      ...anImport(1),
      status: "running",
      stage: "staging_review",
      started_at: "2024-01-01T00:00:00Z",
      finished_at: null,
      summary: null,
      issues: [],
    };
    listAccountImports.mockResolvedValue({ items: [running], total: 1, limit: 50, offset: 0 });
    getAccountImport.mockResolvedValue(running);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<StorageSection />);
    const heading = await screen.findByRole("heading", { name: "Import history" });
    const section = within(heading.parentElement as HTMLElement);
    await user.click(await section.findByRole("button", { expanded: false }));
    const finished = await screen.findByText("Finished");
    const started = screen.getByText("Started");
    expect(finished.nextElementSibling?.textContent).not.toBe(
      started.nextElementSibling?.textContent,
    );
    expect(finished.nextElementSibling?.textContent).toBe("Not finished");
  });
});
