/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { CollapsibleSection } from "./ImportFormUi";

afterEach(cleanup);

describe("CollapsibleSection", () => {
  // The app's own :focus-visible outline loses to the outline-none utility, so
  // the disclosure draws the style guide's ring itself.
  it("draws the focus ring on its disclosure button", () => {
    render(
      <CollapsibleSection title="Attachments" open={false} onToggle={() => {}}>
        <p>Body</p>
      </CollapsibleSection>,
    );
    expect(screen.getByRole("button", { name: /Attachments/ }).className).toContain(
      "focus-visible:ring-2 focus-visible:ring-accent",
    );
  });
});
