/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  localStorage.clear();
  // deviceId.ts caches the id in a module-scoped variable so a real page
  // does not re-read localStorage on every call. Reset the module registry
  // between tests so that in-memory cache does not leak across tests.
  vi.resetModules();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("getDeviceId", () => {
  it("generates an id on first read and keeps it", async () => {
    const { DEVICE_ID_KEY, getDeviceId } = await import("./deviceId");
    const first = getDeviceId();
    expect(first).toMatch(/^[0-9a-f-]{36}$/);
    expect(getDeviceId()).toBe(first);
    expect(localStorage.getItem(DEVICE_ID_KEY)).toBe(first);
  });

  it("makes and keeps an id where the browser has no crypto.randomUUID", async () => {
    // Browsers expose crypto.randomUUID only on HTTPS and localhost, so a
    // Message Crate opened at http://192.168.x.x has none.
    Object.defineProperty(crypto, "randomUUID", { value: undefined, configurable: true });
    try {
      const { DEVICE_ID_KEY, getDeviceId } = await import("./deviceId");
      const first = getDeviceId();
      expect(first).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
      expect(getDeviceId()).toBe(first);
      expect(localStorage.getItem(DEVICE_ID_KEY)).toBe(first);
    } finally {
      // Drop the shadowing property so the real method shows through again.
      delete (crypto as { randomUUID?: unknown }).randomUUID;
    }
  });

  it("reuses an id already stored", async () => {
    const { DEVICE_ID_KEY, getDeviceId } = await import("./deviceId");
    localStorage.setItem(DEVICE_ID_KEY, "11111111-2222-3333-4444-555555555555");
    expect(getDeviceId()).toBe("11111111-2222-3333-4444-555555555555");
  });

  it("keeps an in-memory id for the page when storage throws", async () => {
    // Private browsing and a full quota both throw from the Storage API. The
    // id then lives only in memory: stable within the page, gone on reload.
    const blocked = () => {
      throw new Error("storage disabled");
    };
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(blocked);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(blocked);
    const { DEVICE_ID_KEY, getDeviceId } = await import("./deviceId");

    const first = getDeviceId();
    expect(first).toMatch(/^[0-9a-f-]{36}$/);
    expect(getDeviceId()).toBe(first);

    vi.restoreAllMocks();
    expect(localStorage.getItem(DEVICE_ID_KEY)).toBeNull();
  });
});
