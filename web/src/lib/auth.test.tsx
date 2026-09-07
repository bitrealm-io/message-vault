/** @vitest-environment jsdom */

import { act, render, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { VaultProviders } from "../test/vaultProviders";

const post = vi.fn();
const get = vi.fn();
const getProfile = vi.fn();
const setTokenFn = vi.fn();
const setBaseUrl = vi.fn();
const isTauri = vi.fn();
const onCloseRequested = vi.fn();
const destroy = vi.fn();
const getCurrentWindow = vi.fn();

let currentToken: string | null = null;

vi.mock("./api", () => ({
  setToken: (token: string | null) => {
    currentToken = token;
    setTokenFn(token);
  },
  getToken: () => currentToken,
  setBaseUrl: (...args: unknown[]) => setBaseUrl(...args),
}));

// The vault calls auth.tsx makes, faked by name. Everything else in vaultApi
// stays real, since other modules in this graph import from it.
vi.mock("./vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./vaultApi")>()),
  logout: (...args: unknown[]) => post(...args),
  checkAuth: (...args: unknown[]) => get(...args),
  getAccountProfile: (...args: unknown[]) => getProfile(...args),
}));

vi.mock("./tauri-check", () => ({
  isTauri: () => isTauri(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => getCurrentWindow(),
}));

vi.mock("./contactGroups", () => ({
  invalidateContactGroups: vi.fn(),
}));

vi.mock("./messageTags", () => ({
  invalidateMessageTags: vi.fn(),
}));

const STORAGE_KEY = "message-vault-auth";

function seedSession() {
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      serverUrl: "http://127.0.0.1:8080",
      token: "session-token",
      accountId: "acct-1",
      needsOnboarding: false,
    }),
  );
}

describe("AuthProvider logout", () => {
  beforeEach(() => {
    localStorage.clear();
    currentToken = null;
    post.mockReset();
    get.mockReset();
    setTokenFn.mockReset();
    setBaseUrl.mockReset();
    isTauri.mockReset();
    onCloseRequested.mockReset();
    destroy.mockReset();
    getCurrentWindow.mockReset();

    isTauri.mockReturnValue(false);
    get.mockResolvedValue({ preferred_name: "Sam", phones: ["+1"], emails: [] });
    post.mockResolvedValue({ ok: true });
    destroy.mockResolvedValue(undefined);
    getCurrentWindow.mockReturnValue({
      onCloseRequested,
      destroy,
    });
  });

  it("tells the vault to end the session before clearing the token", async () => {
    seedSession();
    const order: string[] = [];
    post.mockImplementation(async () => {
      order.push("post");
      return { ok: true };
    });
    setTokenFn.mockImplementation((token: string | null) => {
      if (token === null) order.push("clear-token");
    });

    const { AuthProvider, useAuth } = await import("./auth");
    const { result } = renderHook(() => useAuth(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <VaultProviders>
          <AuthProvider>{children}</AuthProvider>
        </VaultProviders>
      ),
    });

    await act(async () => {
      await result.current.logout();
    });

    expect(post).toHaveBeenCalledWith(expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(order.indexOf("post")).toBeGreaterThanOrEqual(0);
    expect(order.indexOf("clear-token")).toBeGreaterThan(order.indexOf("post"));
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull();
    expect(result.current.isAuthenticated).toBe(false);
    expect(result.current.token).toBeNull();
  });

  it("clears the saved login when the vault logout request fails", async () => {
    seedSession();
    post.mockRejectedValue(new Error("network down"));

    const { AuthProvider, useAuth } = await import("./auth");
    const { result } = renderHook(() => useAuth(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <VaultProviders>
          <AuthProvider>{children}</AuthProvider>
        </VaultProviders>
      ),
    });

    await act(async () => {
      await result.current.logout();
    });

    expect(localStorage.getItem(STORAGE_KEY)).toBeNull();
    expect(setTokenFn).toHaveBeenCalledWith(null);
    expect(result.current.isAuthenticated).toBe(false);
  });

  it("skips the vault logout request when there is no session token", async () => {
    const { AuthProvider, useAuth } = await import("./auth");
    const { result } = renderHook(() => useAuth(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <VaultProviders>
          <AuthProvider>{children}</AuthProvider>
        </VaultProviders>
      ),
    });

    await act(async () => {
      await result.current.logout();
    });

    expect(post).not.toHaveBeenCalled();
    expect(result.current.isAuthenticated).toBe(false);
  });

  it("does not register a close handler outside Tauri", async () => {
    isTauri.mockReturnValue(false);
    const { AuthProvider } = await import("./auth");
    render(
      <VaultProviders>
        <AuthProvider>
          <div>ok</div>
        </AuthProvider>
      </VaultProviders>,
    );

    await waitFor(() => {
      expect(getCurrentWindow).not.toHaveBeenCalled();
    });
    expect(onCloseRequested).not.toHaveBeenCalled();
  });

  it("registers a Tauri close handler that logs out then destroys the window", async () => {
    isTauri.mockReturnValue(true);
    let closeHandler: ((event: { preventDefault: () => void }) => Promise<void>) | undefined;
    onCloseRequested.mockImplementation(async (handler) => {
      closeHandler = handler;
      return () => {};
    });

    seedSession();
    const { AuthProvider, useAuth } = await import("./auth");
    const { result } = renderHook(() => useAuth(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <VaultProviders>
          <AuthProvider>{children}</AuthProvider>
        </VaultProviders>
      ),
    });

    await waitFor(() => {
      expect(onCloseRequested).toHaveBeenCalled();
      expect(closeHandler).toBeTypeOf("function");
    });

    const handler = closeHandler;
    if (!handler) {
      throw new Error("expected onCloseRequested handler");
    }
    const preventDefault = vi.fn();
    await act(async () => {
      await handler({ preventDefault });
    });

    expect(preventDefault).toHaveBeenCalled();
    expect(post).toHaveBeenCalledWith(expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull();
    expect(result.current.isAuthenticated).toBe(false);
    expect(destroy).toHaveBeenCalled();
  });

  it("allows another close attempt when destroy fails", async () => {
    isTauri.mockReturnValue(true);
    let closeHandler: ((event: { preventDefault: () => void }) => Promise<void>) | undefined;
    onCloseRequested.mockImplementation(async (handler) => {
      closeHandler = handler;
      return () => {};
    });
    destroy.mockRejectedValueOnce(new Error("denied")).mockResolvedValueOnce(undefined);

    seedSession();
    const { AuthProvider } = await import("./auth");
    render(
      <VaultProviders>
        <AuthProvider>
          <div>ok</div>
        </AuthProvider>
      </VaultProviders>,
    );

    await waitFor(() => {
      expect(closeHandler).toBeTypeOf("function");
    });

    const handler = closeHandler;
    if (!handler) {
      throw new Error("expected onCloseRequested handler");
    }

    await act(async () => {
      await handler({ preventDefault: vi.fn() });
    });
    expect(destroy).toHaveBeenCalledTimes(1);

    await act(async () => {
      await handler({ preventDefault: vi.fn() });
    });
    expect(destroy).toHaveBeenCalledTimes(2);
  });
});
