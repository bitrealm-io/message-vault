/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Providers } from "../../test/providers";
import { DemoAccountCard } from "./DemoAccountCard";

const getDemoAccount = vi.hoisted(() => vi.fn());
const replaceDemoAccount = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 1 }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getDemoAccount: (...a: unknown[]) => getDemoAccount(...a),
  replaceDemoAccount: (...a: unknown[]) => replaceDemoAccount(...a),
}));

const absent = { status: "absent", size: null, error: null };
const ready = { status: "ready", size: null, error: null };
const building = { status: "building", size: "medium", error: null };

function renderCard() {
  return render(
    <Providers>
      <DemoAccountCard />
    </Providers>,
  );
}

beforeEach(() => {
  getDemoAccount.mockReset();
  replaceDemoAccount.mockReset();
  replaceDemoAccount.mockResolvedValue(building);
});

afterEach(cleanup);

describe("DemoAccountCard", () => {
  it("adds the Demo Account at once when there is none, and shows it working", async () => {
    getDemoAccount.mockResolvedValue(absent);
    renderCard();

    await userEvent.click(await screen.findByRole("button", { name: "Add Demo Account" }));

    expect(replaceDemoAccount).toHaveBeenCalledWith({ size: "medium" });
    expect(await screen.findByRole("status")).toHaveTextContent("Building the Demo Account");
    expect(screen.queryByRole("button", { name: /Demo Account$/ })).not.toBeInTheDocument();
  });

  it("asks before resetting, because a reset removes what visitors changed", async () => {
    getDemoAccount.mockResolvedValue(ready);
    renderCard();

    await userEvent.click(await screen.findByRole("button", { name: "Reset Demo Account" }));
    expect(replaceDemoAccount).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("No other account is touched.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Reset Demo Account" }));

    await waitFor(() => expect(replaceDemoAccount).toHaveBeenCalledWith({ size: "medium" }));
  });

  it("keeps reading while a build runs and offers the action again when it ends", async () => {
    getDemoAccount.mockResolvedValueOnce(building).mockResolvedValue(ready);
    renderCard();

    expect(await screen.findByRole("status")).toHaveTextContent("Building the Demo Account");
    expect(
      await screen.findByRole("button", { name: "Reset Demo Account" }, { timeout: 4000 }),
    ).toBeEnabled();
  });

  it("says why the last build failed and offers to add it again", async () => {
    getDemoAccount.mockResolvedValue({ status: "failed", size: null, error: "disk full" });
    renderCard();

    expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
    expect(screen.getByRole("button", { name: "Add Demo Account" })).toBeEnabled();
  });
});
