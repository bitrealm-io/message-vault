/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import ApiTokensTable from "./ApiTokensTable";

afterEach(cleanup);

describe("ApiTokensTable focus", () => {
  // A row sets outline-none, which also removes the app's own :focus-visible
  // outline, so it draws the style guide's ring itself, inside the row.
  it("draws the focus ring on a token's row", () => {
    render(
      <ApiTokensTable
        items={[
          {
            id: 1,
            label: "Phone backup",
            can_import: true,
            can_export: false,
            token_hint: "mc-api-Sd..mE",
            created_at: "1767225600",
          },
        ]}
        busy={false}
        composing={false}
        onRename={() => {}}
        onRevoke={() => {}}
      />,
    );
    const row = screen.getAllByRole("row")[1];
    expect(row.className).toContain(
      "focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent",
    );
  });
});
