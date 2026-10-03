/** @vitest-environment jsdom */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import SourcesPanel from "./SourcesPanel";

vi.mock("../lib/auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

const getSources = vi.fn();

vi.mock("../lib/serverApi", () => ({
  getConversationSources: (...args: unknown[]) => getSources(...args),
}));

describe("SourcesPanel", () => {
  afterEach(() => {
    cleanup();
    getSources.mockReset();
  });

  it("puts each share of unique messages beside the unique count it was computed from", async () => {
    // Two sources with 2 messages each; one message is in both, so the first
    // source holds 2 of the 3 unique messages and the second holds 1.
    getSources.mockResolvedValue([
      { backup_name: "phone-a", message_count: 2, unique_count: 2, percentage: 66.7 },
      { backup_name: "phone-b", message_count: 2, unique_count: 1, percentage: 33.3 },
    ]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={client}>
        <SourcesPanel conversationId={1} onClose={() => {}} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("2 unique (66.7% of unique messages)")).toBeTruthy();
    expect(screen.getByText("1 unique (33.3% of unique messages)")).toBeTruthy();
    expect(screen.getAllByText("2 messages")).toHaveLength(2);
  });
});
