/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { Providers } from "../../test/providers";
import { ServerSettingsPanel } from "./ServerSettingsPanel";

const getServerSettings = vi.hoisted(() => vi.fn());
const updateServerSettings = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 1 }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getServerSettings: (...a: unknown[]) => getServerSettings(...a),
  updateServerSettings: (...a: unknown[]) => updateServerSettings(...a),
  getServerState: () => new Promise(() => {}),
  getDemoAccount: () => new Promise(() => {}),
}));

const MIB = 1024 * 1024;

function renderPanel() {
  return render(
    <Providers>
      <ServerSettingsPanel />
    </Providers>,
  );
}

beforeEach(() => {
  getServerSettings.mockReset();
  getServerSettings.mockResolvedValue({ public_registration: false, asset_max_bytes: 512 * MIB });
  updateServerSettings.mockReset();
});

afterEach(cleanup);

describe("ServerSettingsPanel attachment size limit", () => {
  it("shows the limit the server holds, in megabytes", async () => {
    renderPanel();

    expect(await screen.findByLabelText("Attachment size limit")).toHaveValue(512);
    // Nothing to save until the number differs from the one in force.
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("saves the number the owner types as bytes, and leaves registration out of the request", async () => {
    updateServerSettings.mockResolvedValue({
      public_registration: false,
      asset_max_bytes: 100 * MIB,
    });
    renderPanel();
    const field = await screen.findByLabelText("Attachment size limit");

    await userEvent.clear(field);
    await userEvent.type(field, "100");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(updateServerSettings).toHaveBeenCalledWith({ asset_max_bytes: 100 * MIB });
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
    expect(field).toHaveValue(100);
  });

  it("offers no save for a limit that is blank or not above zero", async () => {
    renderPanel();
    const field = await screen.findByLabelText("Attachment size limit");

    await userEvent.clear(field);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await userEvent.type(field, "0");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(updateServerSettings).not.toHaveBeenCalled();
  });

  it("shows the server's sentence when it refuses the limit, and keeps what was typed", async () => {
    updateServerSettings.mockRejectedValue(
      new ApiError(422, "asset_max_bytes must be at least 67108864 (64 MiB)"),
    );
    renderPanel();
    const field = await screen.findByLabelText("Attachment size limit");

    await userEvent.clear(field);
    await userEvent.type(field, "10");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "asset_max_bytes must be at least 67108864 (64 MiB)",
    );
    expect(field).toHaveValue(10);
  });
});
