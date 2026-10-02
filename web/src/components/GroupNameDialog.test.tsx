/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import GroupNameDialog from "./GroupNameDialog";

afterEach(cleanup);

describe("GroupNameDialog", () => {
  it("stays open on Escape while its save is in flight", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(
      <GroupNameDialog
        title="New Contact Group"
        initial="Family"
        busy
        onSave={() => {}}
        onCancel={onCancel}
      />,
    );

    await user.keyboard("{Escape}");

    expect(screen.getByRole("dialog", { name: "New Contact Group" })).toBeInTheDocument();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("closes on Escape when no save is in flight", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<GroupNameDialog title="New Contact Group" onSave={() => {}} onCancel={onCancel} />);

    await user.keyboard("{Escape}");

    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
