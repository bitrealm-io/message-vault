/** @vitest-environment jsdom */

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useSelectAll } from "./useSelectAll";

describe("useSelectAll", () => {
  it("drops a failure once the list leaves it, and does not show it on coming back", async () => {
    const loadAll = vi.fn().mockRejectedValue(new Error("offset is past the end"));
    const onRows = vi.fn();
    const { result, rerender } = renderHook(
      ({ q }: { q: string }) => useSelectAll(loadAll, [q], onRows),
      { initialProps: { q: "" } },
    );

    await act(() => result.current.selectAll());
    expect(result.current.error).toBe(
      "Select all could not read every row: offset is past the end",
    );

    rerender({ q: "x" });
    expect(result.current.error).toBeNull();
    rerender({ q: "" });
    await waitFor(() => expect(result.current.error).toBeNull());
    expect(onRows).not.toHaveBeenCalled();
  });

  it("drops rows that answer for a list the person has left", async () => {
    let answer!: (rows: number[]) => void;
    const loadAll = vi.fn(
      () =>
        new Promise<number[]>((resolve) => {
          answer = resolve;
        }),
    );
    const onRows = vi.fn();
    const { result, rerender } = renderHook(
      ({ q }: { q: string }) => useSelectAll(loadAll, [q], onRows),
      { initialProps: { q: "" } },
    );

    let pending!: Promise<void>;
    act(() => {
      pending = result.current.selectAll();
    });
    expect(result.current.selecting).toBe(true);
    rerender({ q: "x" });
    expect(result.current.selecting).toBe(false);

    await act(async () => {
      answer([1, 2, 3]);
      await pending;
    });
    expect(onRows).not.toHaveBeenCalled();
  });
});
