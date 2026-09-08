/** @vitest-environment jsdom */

/**
 * The profile is one entry that every screen reads and two screens write.
 *
 * A write answers with the whole profile, so it belongs in that entry
 * directly: asking the vault again would show the old name for as long as the
 * round trip takes.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAccountProfile, useUpdateAccountProfile } from "./useAccountProfile";
import { getAccountProfile, updateAccountProfile } from "./vaultApi";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./vaultApi")>()),
  getAccountProfile: vi.fn(),
  updateAccountProfile: vi.fn(),
}));

const read = vi.mocked(getAccountProfile);
const write = vi.mocked(updateAccountProfile);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function profile(name: string) {
  return { preferred_name: name, phones: [], emails: [] } as unknown as Awaited<
    ReturnType<typeof getAccountProfile>
  >;
}

beforeEach(() => {
  vi.clearAllMocks();
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
});

describe("useUpdateAccountProfile", () => {
  it("shows the answered profile without asking the vault again", async () => {
    read.mockResolvedValue(profile("Ada"));
    write.mockResolvedValue(profile("Ada Lovelace"));

    const both = renderHook(
      () => ({ profile: useAccountProfile(), update: useUpdateAccountProfile() }),
      { wrapper },
    );
    await waitFor(() => expect(both.result.current.profile.profile?.preferred_name).toBe("Ada"));
    expect(read).toHaveBeenCalledTimes(1);

    await both.result.current.update.mutateAsync({ preferred_name: "Ada Lovelace" });

    expect(write).toHaveBeenCalledWith({ preferred_name: "Ada Lovelace" });
    await waitFor(() =>
      expect(both.result.current.profile.profile?.preferred_name).toBe("Ada Lovelace"),
    );
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("leaves the profile alone when the vault refuses", async () => {
    read.mockResolvedValue(profile("Ada"));
    write.mockRejectedValue(new Error("that address is already claimed"));

    const both = renderHook(
      () => ({ profile: useAccountProfile(), update: useUpdateAccountProfile() }),
      { wrapper },
    );
    await waitFor(() => expect(both.result.current.profile.profile?.preferred_name).toBe("Ada"));

    await expect(
      both.result.current.update.mutateAsync({
        handles: [{ handle: "+15550000", service: "phone" }],
      }),
    ).rejects.toThrow("that address is already claimed");
    expect(both.result.current.profile.profile?.preferred_name).toBe("Ada");
  });
});
