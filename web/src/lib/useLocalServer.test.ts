/** @vitest-environment jsdom */

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { READY_POLL_MS, useLocalServer } from "./useLocalServer";

const startLocalServer = vi.hoisted(() => vi.fn());
const localServerStatus = vi.hoisted(() => vi.fn());

vi.mock("./localServer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./localServer")>()),
  startLocalServer: () => startLocalServer(),
  localServerStatus: () => localServerStatus(),
}));

beforeEach(() => {
  vi.useFakeTimers();
  startLocalServer.mockReset();
  localServerStatus.mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useLocalServer", () => {
  it("notices when a Message Crate the app found stops answering", async () => {
    startLocalServer.mockResolvedValue({ status: "ready", started_by_app: false });
    localServerStatus.mockResolvedValue({ status: "ready", started_by_app: false });
    const { result } = renderHook(() => useLocalServer(true));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(result.current.status).toEqual({ status: "ready", started_by_app: false });

    // Docker is stopped, and the app starts its own server in its place.
    localServerStatus.mockResolvedValue({ status: "starting", first_time: false });
    await act(() => vi.advanceTimersByTimeAsync(READY_POLL_MS));

    expect(result.current.status).toEqual({ status: "starting", first_time: false });
  });

  it("stops asking once it is no longer active", async () => {
    startLocalServer.mockResolvedValue({ status: "ready", started_by_app: true });
    localServerStatus.mockResolvedValue({ status: "ready", started_by_app: true });
    const { rerender } = renderHook(({ active }) => useLocalServer(active), {
      initialProps: { active: true },
    });
    await act(() => vi.advanceTimersByTimeAsync(0));

    rerender({ active: false });
    await act(() => vi.advanceTimersByTimeAsync(READY_POLL_MS * 3));

    expect(localServerStatus).not.toHaveBeenCalled();
  });
});
