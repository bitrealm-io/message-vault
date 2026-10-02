/** @vitest-environment jsdom */

/**
 * The profile is one entry that every screen reads and two screens write.
 *
 * A write answers with the whole profile, so it belongs in that entry
 * directly: waiting for the server to be asked again would show the old name
 * for as long as the round trip takes.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getAccountProfile, updateAccountProfile } from "./serverApi";
import { useAccountProfile, useUpdateAccountProfile } from "./useAccountProfile";

vi.mock("./auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

vi.mock("./serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./serverApi")>()),
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
  it("shows the answered profile without waiting for the server to be asked again", async () => {
    read.mockResolvedValueOnce(profile("Ada"));
    // The read every write starts never answers, so only the write's own
    // answer can show the new name.
    read.mockReturnValue(new Promise(() => {}));
    write.mockResolvedValue(profile("Ada Lovelace"));

    const both = renderHook(
      () => ({ profile: useAccountProfile(), update: useUpdateAccountProfile() }),
      { wrapper },
    );
    await waitFor(() => expect(both.result.current.profile.profile?.preferred_name).toBe("Ada"));

    await both.result.current.update.mutateAsync({ preferred_name: "Ada Lovelace" });

    expect(write).toHaveBeenCalledWith({ preferred_name: "Ada Lovelace" });
    await waitFor(() =>
      expect(both.result.current.profile.profile?.preferred_name).toBe("Ada Lovelace"),
    );
  });

  it("leaves the profile alone when the server refuses", async () => {
    read.mockResolvedValue(profile("Ada"));
    write.mockRejectedValue(new Error("that address is already claimed"));

    const both = renderHook(
      () => ({ profile: useAccountProfile(), update: useUpdateAccountProfile() }),
      { wrapper },
    );
    await waitFor(() => expect(both.result.current.profile.profile?.preferred_name).toBe("Ada"));

    await expect(
      both.result.current.update.mutateAsync({
        identities: [{ address: "+15550000", service: "phone" }],
      }),
    ).rejects.toThrow("that address is already claimed");
    expect(both.result.current.profile.profile?.preferred_name).toBe("Ada");
  });
});
