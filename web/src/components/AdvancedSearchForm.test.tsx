/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import AdvancedSearchForm from "./AdvancedSearchForm";

afterEach(cleanup);

describe("AdvancedSearchForm on Messages", () => {
  it("offers every source an import writes, by the name a person knows it by", async () => {
    const user = userEvent.setup();
    render(<AdvancedSearchForm mode="messages" onApply={vi.fn()} onClose={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /Source/ }));

    const names = screen.getAllByRole("option").map((o) => o.textContent?.replace("✓", "").trim());
    expect(names).toEqual([
      "iMessage",
      "WhatsApp",
      "SMS Backup & Restore",
      "GO SMS Pro",
      "iMazing",
      "SMS Backup+",
      "OpenExtract",
    ]);
  });

  it("searches the ticked sources by the id an import writes", async () => {
    const user = userEvent.setup();
    const onApply = vi.fn();
    render(<AdvancedSearchForm mode="messages" onApply={onApply} onClose={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /Source/ }));
    await user.click(screen.getByRole("option", { name: "iMazing" }));
    await user.click(screen.getByRole("option", { name: "GO SMS Pro" }));
    await user.click(screen.getByRole("button", { name: "Search" }));

    expect(onApply).toHaveBeenCalledWith("source:imazing,go-sms-pro");
  });
});
