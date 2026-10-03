/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import DateField from "./DateField";

afterEach(cleanup);

const ring = "focus-visible:ring-2 focus-visible:ring-accent";

// The app's own :focus-visible outline loses to the outline-none utility, so
// each button of the field draws the style guide's ring itself.
describe("DateField", () => {
  it("draws the focus ring on the calendar button and the calendar's month buttons", async () => {
    render(<DateField label="From" value="2024-03-15" onChange={() => {}} />);

    const pick = screen.getByRole("button", { name: /Pick From/ });
    expect(pick.className).toContain(ring);

    await userEvent.click(pick);
    await screen.findAllByRole("button", { name: "Previous" });
    const months = document.querySelectorAll('button[slot="previous"], button[slot="next"]');
    expect(months).toHaveLength(2);
    for (const button of months) {
      expect(button.className).toContain(ring);
    }
  });
});
