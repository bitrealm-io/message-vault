/** @vitest-environment jsdom */

import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const useAccountProfile = vi.fn();

vi.mock("./useAccountProfile", () => ({
  useAccountProfile: () => useAccountProfile(),
}));

import { useNeedsProfileSetup } from "./useNeedsProfileSetup";

/**
 * Whether an account still owes profile setup is the vault's answer, read off
 * the profile. These pin that the hook reports what the server said and draws
 * no conclusion of its own — the browser used to infer it from a profile that
 * looked empty, and two clients inferring separately is how the rule drifted.
 */
describe("useNeedsProfileSetup", () => {
  beforeEach(() => {
    useAccountProfile.mockReset();
  });

  it("reports the flag the vault set", () => {
    useAccountProfile.mockReturnValue({
      profile: { must_set_up_profile: true },
      loading: false,
    });

    const { result } = renderHook(() => useNeedsProfileSetup());
    expect(result.current).toEqual({ needsSetup: true, loading: false });
  });

  it("leaves an account the vault says is set up alone", () => {
    useAccountProfile.mockReturnValue({
      profile: { must_set_up_profile: false },
      loading: false,
    });

    const { result } = renderHook(() => useNeedsProfileSetup());
    expect(result.current.needsSetup).toBe(false);
  });

  // An empty-looking profile is exactly what the browser used to read as
  // "needs setup". It is not the question any more: the vault answers it.
  it("does not infer setup from a profile with nothing in it", () => {
    useAccountProfile.mockReturnValue({
      profile: { must_set_up_profile: false, preferred_name: null, phones: [], emails: [] },
      loading: false,
    });

    const { result } = renderHook(() => useNeedsProfileSetup());
    expect(result.current.needsSetup).toBe(false);
  });

  it("owes nothing while the profile has not loaded, and says it is loading", () => {
    useAccountProfile.mockReturnValue({ profile: null, loading: true });

    const { result } = renderHook(() => useNeedsProfileSetup());
    expect(result.current).toEqual({ needsSetup: false, loading: true });
  });
});
