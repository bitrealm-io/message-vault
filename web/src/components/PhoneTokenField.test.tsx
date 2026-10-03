/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import PhoneTokenField from "./PhoneTokenField";

afterEach(cleanup);

describe("PhoneTokenField focus", () => {
  // A number sets outline-none, which also removes the app's own
  // :focus-visible outline. The field's border shows that something inside it
  // has focus but not which number, so the number draws the ring itself.
  it("draws the focus ring on a number reached with the keyboard", () => {
    render(
      <PhoneTokenField value={["555-0101", "555-0102"]} onChange={() => {}} aria-label="Phones" />,
    );
    const numbers = screen.getAllByRole("row");
    expect(numbers).toHaveLength(2);
    for (const number of numbers) {
      expect(number.className).toContain("focus-visible:ring-2 focus-visible:ring-accent");
    }
  });
});
