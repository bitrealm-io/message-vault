/** @vitest-environment jsdom */

import { cleanup, render as rtlRender, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement, ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiTokenRevealProvider } from "../../components/ApiTokenRevealDialog";
import { mockedAuth, Providers } from "../../test/providers";
import { ApiTokensSection } from "./ApiTokensSection";

/** The app renders the reveal dialog above Settings, so the tests do too. */
const render = (ui: ReactElement | null) =>
  rtlRender(ui, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <Providers>
        <ApiTokenRevealProvider>{children}</ApiTokenRevealProvider>
      </Providers>
    ),
  });

const apiGet = vi.hoisted(() => vi.fn());
const apiPost = vi.hoisted(() => vi.fn());

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listApiTokens: (...args: unknown[]) => apiGet(...args),
  createApiToken: (...args: unknown[]) => apiPost(...args),
  renameApiToken: vi.fn(),
  deleteApiToken: vi.fn(),
}));

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  apiGet.mockReset();
  apiPost.mockReset();
  apiGet.mockResolvedValue([]);
});

async function openComposeForm() {
  const user = userEvent.setup();
  render(<ApiTokensSection accountCanImport={true} accountCanExport={true} />);
  await waitFor(() => {
    expect(apiGet).toHaveBeenCalled();
  });
  await user.click(screen.getByRole("button", { name: "Add" }));
  return user;
}

describe("ApiTokensSection create form", () => {
  it("sends exactly label, can_import, can_export as the request body", async () => {
    apiPost.mockResolvedValue({
      id: "tok_1",
      label: "My token",
      can_import: true,
      can_export: true,
      created_at: "1700000000",
      token: "mc-api-secret",
      token_hint: "mc-api-se..et",
    });

    const user = await openComposeForm();
    await user.type(screen.getByLabelText("API Token name"), "My token");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(apiPost).toHaveBeenCalledTimes(1);
    });
    const [body] = apiPost.mock.calls[0] as [unknown];
    expect(body).toEqual({
      label: "My token",
      can_import: true,
      can_export: true,
    });
  });

  it("offers no delete permission, because a token never carries one", async () => {
    await openComposeForm();
    expect(screen.queryByRole("checkbox", { name: /delete/i })).toBeNull();
  });

  it("disables a permission checkbox the account itself does not hold", async () => {
    const user = userEvent.setup();
    render(<ApiTokensSection accountCanImport={false} accountCanExport={true} />);
    await waitFor(() => {
      expect(apiGet).toHaveBeenCalled();
    });
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(screen.getByRole("checkbox", { name: "Import" })).toBeDisabled();
    expect(screen.getByText("Your account cannot do this.")).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Export" })).not.toBeDisabled();
  });

  it("forces a permission checkbox unchecked when the account lacks it, even though the form defaults it on", async () => {
    const user = userEvent.setup();
    render(<ApiTokensSection accountCanImport={false} accountCanExport={true} />);
    await waitFor(() => {
      expect(apiGet).toHaveBeenCalled();
    });
    await user.click(screen.getByRole("button", { name: "Add" }));

    // The form defaults `canImport` to true, but the account cannot import —
    // the box must read unchecked, not checked-but-disabled.
    const importCheckbox = screen.getByRole("checkbox", { name: "Import" });
    expect(importCheckbox).toBeDisabled();
    expect(importCheckbox).not.toBeChecked();

    apiPost.mockResolvedValue({
      id: "tok_3",
      label: "No import",
      can_import: false,
      can_export: true,
      created_at: "1700000000",
      token: "mc-api-secret3",
      token_hint: "mc-api-se..t3",
    });
    await user.type(screen.getByLabelText("API Token name"), "No import");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(apiPost).toHaveBeenCalledTimes(1);
    });
    expect(apiPost.mock.calls[0][0]).toEqual({
      label: "No import",
      can_import: false,
      can_export: true,
    });
  });
});

describe("ApiTokensSection table", () => {
  const expiring = {
    id: 1,
    label: "Laptop",
    can_import: true,
    can_export: true,
    token_hint: "mc-api-la..op",
    created_at: "1700000000",
    last_accessed_at: "1700086400",
    expires_at: "1731536000",
    disabled: false,
  };
  const unending = {
    id: 2,
    label: "Backup script",
    can_import: false,
    can_export: true,
    token_hint: "mc-api-ba..pt",
    created_at: "1700000000",
    last_accessed_at: "1700086400",
    disabled: false,
  };

  it("shows when each token expires, and Never for one that does not", async () => {
    apiGet.mockResolvedValue([expiring, unending]);
    render(<ApiTokensSection accountCanImport={true} accountCanExport={true} />);

    const rowOf = (label: string) => screen.getByText(label).closest("tr") as HTMLElement;
    await screen.findByText("Laptop");
    expect(screen.getByRole("columnheader", { name: "Expires" })).toBeTruthy();
    const expiry = new Date(1731536000 * 1000).toLocaleDateString();
    const expiringRow = rowOf("Laptop");
    expect(within(expiringRow).getByText(expiry)).toBeTruthy();
    expect(within(expiringRow).queryByText("Never")).toBeNull();
    const unendingRow = rowOf("Backup script");
    expect(within(unendingRow).getByText("Never")).toBeTruthy();
  });

  it("says API Token everywhere, never API key, and promises no delete", async () => {
    apiGet.mockResolvedValue([expiring]);
    apiPost.mockResolvedValue({
      id: 3,
      label: "Phone",
      can_import: true,
      can_export: true,
      created_at: "1700000000",
      token: "mc-api-secret",
      token_hint: "mc-api-se..et",
    });
    const user = userEvent.setup();
    render(<ApiTokensSection accountCanImport={true} accountCanExport={true} />);
    await screen.findByText("Laptop");

    // Markup, not only visible text: labels and tooltips are read out too.
    const wording = () => document.body.innerHTML;
    expect(screen.getByRole("columnheader", { name: "Token" })).toBeTruthy();
    expect(wording()).not.toMatch(/API keys?|(this|delete) key|delete message data/i);

    await user.click(screen.getByRole("button", { name: "Revoke API Token" }));
    expect(await screen.findByText(/Programs using it will stop working/)).toBeTruthy();
    expect(wording()).not.toMatch(/API keys?|(this|delete) key|CLI tools/i);
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    await user.click(screen.getByRole("button", { name: "Edit API Token" }));
    expect(await screen.findByRole("dialog", { name: "Rename API Token" })).toBeTruthy();
    expect(wording()).not.toMatch(/API keys?|(this|delete) key/i);
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(screen.getByLabelText("API Token name"), "Phone");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("dialog", { name: "API Token created" })).toBeTruthy();
    expect(wording()).not.toMatch(/API keys?|(this|delete) key/i);
  });
});

describe("ApiTokensSection reveal", () => {
  it("shows the new secret when the server answers after Settings was left", async () => {
    let answer: (value: unknown) => void = () => {};
    apiPost.mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    const user = userEvent.setup();
    // Switching to another Settings tab, or leaving Settings, unmounts the section.
    const screenWith = (section: boolean) =>
      section ? <ApiTokensSection accountCanImport={true} accountCanExport={true} /> : null;
    const { rerender } = render(screenWith(true));
    await waitFor(() => expect(apiGet).toHaveBeenCalled());
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(screen.getByLabelText("API Token name"), "Phone");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(apiPost).toHaveBeenCalled());

    rerender(screenWith(false));
    answer({
      id: 2,
      label: "Phone",
      can_import: true,
      can_export: true,
      created_at: "1700000000",
      token_hint: "mc-api-ph..ne",
      token: "mc-api-phone-secret",
    });

    const dialog = await screen.findByRole("dialog", { name: "API Token created" });
    expect(within(dialog).getByText("mc-api-phone-secret")).toBeTruthy();
  });
});
