/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../../test/providers";
import { ManagedApiTokensSection } from "./ManagedApiTokensSection";

const apiList = vi.hoisted(() => vi.fn());
const apiDelete = vi.hoisted(() => vi.fn());

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listApiTokens: (...args: unknown[]) => apiList(...args),
  deleteApiToken: (...args: unknown[]) => apiDelete(...args),
  createApiToken: vi.fn(),
  renameApiToken: vi.fn(),
}));

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  apiList.mockReset();
  apiDelete.mockReset();
  // The owner's list carries no `token_hint`: the server leaves it out.
  apiList.mockResolvedValue({
    items: [
      {
        id: 3,
        label: "Backup script",
        can_import: true,
        can_export: false,
        created_at: "1700000000",
        last_accessed_at: "1700086400",
        disabled: false,
      },
    ],
  });
  apiDelete.mockResolvedValue(undefined);
});

describe("ManagedApiTokensSection", () => {
  it("lists the account's tokens with no secret and no rename", async () => {
    render(<ManagedApiTokensSection accountId={12} />, { wrapper: Providers });

    const row = await screen.findByRole("row", { name: /Backup script/ });
    expect(apiList.mock.calls[0]?.[1]).toBe(12);
    expect(within(row).getByText("Import")).toBeTruthy();
    expect(screen.queryByRole("columnheader", { name: "Token" })).toBeNull();
    expect(screen.queryByText(/mc-api-/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Edit API Token" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Add" })).toBeNull();
  });

  it("revokes a token of the account it was opened for", async () => {
    const user = userEvent.setup();
    render(<ManagedApiTokensSection accountId={12} />, { wrapper: Providers });

    await user.click(await screen.findByRole("button", { name: "Revoke API Token" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/Backup script/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => {
      expect(apiDelete).toHaveBeenCalledWith(3, 12);
    });
  });
});
