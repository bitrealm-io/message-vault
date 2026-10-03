/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import PlainButton from "./PlainButton";

afterEach(cleanup);

describe("PlainButton", () => {
  it("keeps the hover text React Aria's Button drops", () => {
    render(<PlainButton title="Sorted by Name">Sort</PlainButton>);
    expect(screen.getByRole("button", { name: "Sort" })).toHaveAttribute("title", "Sorted by Name");
  });

  it("marks keyboard focus, and runs onPress from the keyboard", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(<PlainButton onPress={onPress}>Export</PlainButton>);
    const button = screen.getByRole("button", { name: "Export" });

    await user.tab();
    expect(button).toHaveAttribute("data-focus-visible", "true");

    await user.keyboard("{Enter}");
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it("does not let a press reach a click handler around it", async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    const onPress = vi.fn();
    // As in the import history, the row toggles on a click and the button in it does too.
    render(
      <table>
        <tbody>
          <tr onClick={onRowClick}>
            <td>
              <PlainButton onPress={onPress}>Open</PlainButton>
            </td>
          </tr>
        </tbody>
      </table>,
    );

    await user.click(screen.getByRole("button", { name: "Open" }));

    expect(onPress).toHaveBeenCalledTimes(1);
    expect(onRowClick).not.toHaveBeenCalled();
  });
});
