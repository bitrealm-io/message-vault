/** @vitest-environment jsdom */

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  checkServerHealth,
  HEALTH_BACKOFF_CAP_MS,
  HEALTH_PROBE_TIMEOUT_MS,
  healthBackoffMs,
  healthProbeUrl,
  healthStatusLabel,
} from "./serverHealth";

describe("healthBackoffMs", () => {
  it("grows then caps at 30s", () => {
    expect(healthBackoffMs(0)).toBe(1000);
    expect(healthBackoffMs(1)).toBe(2000);
    expect(healthBackoffMs(2)).toBe(4000);
    expect(healthBackoffMs(3)).toBe(8000);
    expect(healthBackoffMs(4)).toBe(16000);
    expect(healthBackoffMs(5)).toBe(HEALTH_BACKOFF_CAP_MS);
    expect(healthBackoffMs(10)).toBe(HEALTH_BACKOFF_CAP_MS);
  });

  it("is monotonic for successive failure indexes", () => {
    for (let i = 0; i < 8; i++) {
      expect(healthBackoffMs(i + 1)).toBeGreaterThanOrEqual(healthBackoffMs(i));
    }
  });
});

describe("healthProbeUrl", () => {
  it("uses this origin when the URL is blank", () => {
    expect(healthProbeUrl("")).toBe("/health");
    expect(healthProbeUrl("   ")).toBe("/health");
  });

  it("strips trailing slashes on absolute http(s) URLs", () => {
    expect(healthProbeUrl("http://127.0.0.1:8080/")).toBe("http://127.0.0.1:8080/health");
    expect(healthProbeUrl("https://vault.example.com/app/")).toBe(
      "https://vault.example.com/app/health",
    );
  });

  it("rejects relative and non-http values", () => {
    expect(healthProbeUrl("hello")).toBeNull();
    expect(healthProbeUrl("ftp://example.com")).toBeNull();
    expect(healthProbeUrl("javascript:alert(1)")).toBeNull();
  });
});

describe("healthStatusLabel", () => {
  it("uses one vocabulary: connecting, connected, disconnected", () => {
    expect(healthStatusLabel("ok")).toBe("Connected");
    expect(healthStatusLabel("fail")).toBe("Disconnected");
    // Not yet answered and still trying read the same from the user's side.
    expect(healthStatusLabel("checking")).toBe("Connecting…");
    expect(healthStatusLabel("unknown")).toBe("Connecting…");
  });
});

describe("checkServerHealth", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("returns true when /health is ok", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    await expect(checkServerHealth("http://127.0.0.1:8080/")).resolves.toBe(true);
    expect(fetchMock).toHaveBeenCalledWith("http://127.0.0.1:8080/health", {
      method: "GET",
      signal: expect.any(AbortSignal),
      cache: "no-store",
    });
  });

  it("probes this origin when the URL is blank", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);
    await expect(checkServerHealth("   ")).resolves.toBe(true);
    expect(fetchMock).toHaveBeenCalledWith("/health", {
      method: "GET",
      signal: expect.any(AbortSignal),
      cache: "no-store",
    });
  });

  it("returns false when /health is not ok", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 503 }));
    await expect(checkServerHealth("http://127.0.0.1:8080")).resolves.toBe(false);
  });

  it("returns false on network error", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    await expect(checkServerHealth("http://127.0.0.1:8080")).resolves.toBe(false);
  });

  it("returns false for a non-http URL without calling fetch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    await expect(checkServerHealth("hello")).resolves.toBe(false);
    await expect(checkServerHealth("ftp://example.com")).resolves.toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns false when aborted", async () => {
    const fetchMock = vi.fn().mockRejectedValue(new DOMException("Aborted", "AbortError"));
    vi.stubGlobal("fetch", fetchMock);
    const ac = new AbortController();
    ac.abort();
    await expect(checkServerHealth("http://127.0.0.1:8080", ac.signal)).resolves.toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns false when the probe times out", async () => {
    vi.useFakeTimers();
    const fetchMock = vi.fn((_url: string, init?: RequestInit) => {
      return new Promise((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => {
          reject(new DOMException("Aborted", "AbortError"));
        });
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    const pending = checkServerHealth("http://127.0.0.1:8080");
    await vi.advanceTimersByTimeAsync(HEALTH_PROBE_TIMEOUT_MS);
    await expect(pending).resolves.toBe(false);
  });
});
