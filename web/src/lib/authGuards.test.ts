import { describe, expect, it } from "vitest";
import {
  DEFAULT_TAURI_VAULT_URL,
  initialLoginServerUrl,
  parsePersistedAuth,
} from "./authGuards.ts";

describe("initialLoginServerUrl", () => {
  it("defaults the desktop app to IPv4 loopback", () => {
    expect(initialLoginServerUrl(undefined, true)).toBe(DEFAULT_TAURI_VAULT_URL);
    expect(initialLoginServerUrl("", true)).toBe(DEFAULT_TAURI_VAULT_URL);
  });

  it("leaves the browser field blank so the page origin is used", () => {
    expect(initialLoginServerUrl(undefined, false)).toBe("");
    expect(initialLoginServerUrl("", false)).toBe("");
  });

  it("rewrites the old localhost default and keeps any other saved URL", () => {
    expect(initialLoginServerUrl("http://localhost:8080", true)).toBe(DEFAULT_TAURI_VAULT_URL);
    expect(initialLoginServerUrl("http://localhost:8080/", false)).toBe(DEFAULT_TAURI_VAULT_URL);
    expect(initialLoginServerUrl("https://vault.example.com", true)).toBe(
      "https://vault.example.com",
    );
  });
});

describe("parsePersistedAuth", () => {
  it("parses valid persisted auth", () => {
    expect(
      parsePersistedAuth(
        JSON.stringify({
          serverUrl: "http://localhost:8080",
          token: "tok",
          accountId: "acc1",
        }),
      ),
    ).toEqual({
      serverUrl: "http://localhost:8080",
      token: "tok",
      accountId: "acc1",
    });
  });

  it("drops a needs-setup flag left in storage by an older build", () => {
    expect(
      parsePersistedAuth(
        JSON.stringify({
          serverUrl: "http://localhost:8080",
          token: "tok",
          accountId: "acc1",
          needsOnboarding: true,
        }),
      ),
    ).toEqual({
      serverUrl: "http://localhost:8080",
      token: "tok",
      accountId: "acc1",
    });
  });

  it("returns null for corrupt or incomplete JSON", () => {
    expect(parsePersistedAuth("{")).toBeNull();
    expect(parsePersistedAuth("not json")).toBeNull();
    expect(parsePersistedAuth(JSON.stringify({ token: "t" }))).toBeNull();
    expect(
      parsePersistedAuth(JSON.stringify({ serverUrl: "", token: "", accountId: "" })),
    ).toBeNull();
  });
});
