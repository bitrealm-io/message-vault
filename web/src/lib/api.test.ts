import { afterEach, describe, expect, it, vi } from "vitest";
import { apiClient, getBaseUrl, problemFromBody, setBaseUrl, setToken, VaultApiError } from "./api";

afterEach(() => {
  vi.unstubAllGlobals();
  setBaseUrl("");
  setToken(null);
});

/** Stub fetch with a 200 and an empty JSON body, returning the spy. */
function stubOkFetch() {
  const fetchSpy = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({}),
    text: async () => "{}",
  });
  vi.stubGlobal("fetch", fetchSpy);
  return fetchSpy;
}

/** The `[url, init]` pair the client passed to fetch. */
function lastCall(fetchSpy: ReturnType<typeof vi.fn>): [string, RequestInit] {
  const call = fetchSpy.mock.calls.at(-1);
  if (!call) throw new Error("fetch was never called");
  return call as [string, RequestInit];
}

const PROBLEM = {
  type: "https://bitrealm.io/vault/developer/reference/errors/invalid-credentials",
  title: "Invalid credentials",
  status: 401,
  detail: "invalid username or password",
  request_id: "3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c",
};

describe("problemFromBody", () => {
  it("reads the sentence, the slug and the request id out of a problem document", () => {
    const err = problemFromBody(401, JSON.stringify(PROBLEM));
    expect(err.message).toBe("invalid username or password");
    expect(err.type).toBe("invalid-credentials");
    expect(err.title).toBe("Invalid credentials");
    expect(err.requestId).toBe("3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c");
    expect(err.status).toBe(401);
  });

  it("joins every error of a validation failure into the message", () => {
    const err = problemFromBody(
      422,
      JSON.stringify({
        type: "https://bitrealm.io/vault/developer/reference/errors/validation-failed",
        title: "Validation failed",
        status: 422,
        errors: ["limit must be at least 1", "offset exceeds maximum of 50000"],
      }),
    );
    expect(err.message).toBe("limit must be at least 1; offset exceeds maximum of 50000");
    expect(err.errors).toEqual(["limit must be at least 1", "offset exceeds maximum of 50000"]);
    expect(err.type).toBe("validation-failed");
  });

  it("gives an internal error no slug", () => {
    const err = problemFromBody(
      500,
      JSON.stringify({ type: "about:blank", title: "Internal server error", status: 500 }),
    );
    expect(err.type).toBeNull();
    expect(err.message).toBe("Internal server error");
  });

  it("falls back to the raw body when it is not a problem", () => {
    const err = problemFromBody(502, "<html>Bad Gateway</html>");
    expect(err.message).toBe("<html>Bad Gateway</html>");
    expect(err.type).toBeNull();
  });

  it("falls back to a generic sentence for an empty body", () => {
    expect(problemFromBody(500, "   ").message).toBe("Request failed (500)");
  });

  it("clamps an oversized raw-text fallback so it cannot overrun the fixed card", () => {
    const html = `<html><body>${"x".repeat(500)}</body></html>`;
    const message = problemFromBody(502, html).message;
    expect(message.length).toBe(201);
    expect(message.endsWith("…")).toBe(true);
    expect(message.startsWith(html.slice(0, 200))).toBe(true);
  });

  it("leaves a short raw-text fallback whole", () => {
    const short = "<html>Bad Gateway</html>";
    expect(problemFromBody(502, short).message).toBe(short);
  });
});

describe("apiClient errors", () => {
  it("throws a VaultApiError carrying the status and the server's message", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        text: async () =>
          JSON.stringify({
            type: "https://bitrealm.io/vault/developer/reference/errors/username-taken",
            title: "Username taken",
            status: 409,
            detail: "username already taken: matt",
          }),
      }),
    );

    await expect(apiClient.post("/v1/accounts", {})).rejects.toMatchObject({
      name: "VaultApiError",
      status: 409,
      message: "username already taken: matt",
    });
  });

  it("is an Error, so existing catch blocks keep working", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 401,
        text: async () => JSON.stringify({ ...PROBLEM }),
      }),
    );

    const caught = await apiClient.get("/v1/whoami").catch((e: unknown) => e);
    expect(caught).toBeInstanceOf(Error);
    expect(caught).toBeInstanceOf(VaultApiError);
  });
});

describe("apiClient no-content", () => {
  it("resolves undefined for a 204 rather than parsing an empty body", async () => {
    const json = vi.fn().mockRejectedValue(new SyntaxError("Unexpected end of JSON input"));
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, status: 204, json, text: async () => "" }),
    );
    await expect(apiClient.delete("/v1/contact-groups/7")).resolves.toBeUndefined();
    expect(json).not.toHaveBeenCalled();
  });
});

/**
 * The request the client actually sends.
 *
 * Everything here was untested: the file's tests stubbed fetch and read the
 * response, so the URL, the Authorization header, the media type and the body
 * were never looked at. A client that dropped the Bearer token, sent the body
 * as `[object Object]`, or built the URL without the base would have passed
 * every one of them, and every screen would have failed against a real vault.
 */
describe("apiClient request shape", () => {
  it("puts the path after the base URL", async () => {
    const fetchSpy = stubOkFetch();
    setBaseUrl("https://vault.example.test");

    await apiClient.get("/v1/conversations");

    const [url] = lastCall(fetchSpy);
    expect(url).toBe("https://vault.example.test/v1/conversations");
  });

  it("strips trailing slashes off the base URL so the path is not doubled", () => {
    setBaseUrl("https://vault.example.test///");
    expect(getBaseUrl()).toBe("https://vault.example.test");
  });

  it("sends no host of its own when the base URL is empty, so the page's host serves the API", async () => {
    const fetchSpy = stubOkFetch();
    setBaseUrl("");

    await apiClient.get("/v1/conversations");

    expect(lastCall(fetchSpy)[0]).toBe("/v1/conversations");
  });

  it("carries the session token as a Bearer header once one is set", async () => {
    const fetchSpy = stubOkFetch();
    setToken("mv-user-abc123");

    await apiClient.get("/v1/session");

    const headers = lastCall(fetchSpy)[1].headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer mv-user-abc123");
  });

  it("sends no Authorization header while signed out", async () => {
    const fetchSpy = stubOkFetch();
    setToken(null);

    await apiClient.post("/v1/session", { username: "matt" });

    const headers = lastCall(fetchSpy)[1].headers as Record<string, string>;
    expect(headers.Authorization).toBeUndefined();
  });

  it("serializes the body as JSON under the JSON media type", async () => {
    const fetchSpy = stubOkFetch();

    await apiClient.post("/v1/contact-groups", { name: "Family", contact_ids: [1, 2] });

    const [, init] = lastCall(fetchSpy);
    expect(init.method).toBe("POST");
    const headers = init.headers as Record<string, string>;
    expect(headers["Content-Type"]).toBe("application/json");
    expect(JSON.parse(init.body as string)).toEqual({
      name: "Family",
      contact_ids: [1, 2],
    });
  });

  it('sends no body at all when there is none, rather than the string "undefined"', async () => {
    const fetchSpy = stubOkFetch();

    await apiClient.post("/v1/session/refresh");

    expect(lastCall(fetchSpy)[1].body).toBeUndefined();
  });

  it("uses the method the verb names", async () => {
    const fetchSpy = stubOkFetch();

    await apiClient.get("/v1/a");
    expect(lastCall(fetchSpy)[1].method).toBe("GET");
    await apiClient.post("/v1/a");
    expect(lastCall(fetchSpy)[1].method).toBe("POST");
    await apiClient.put("/v1/a", {});
    expect(lastCall(fetchSpy)[1].method).toBe("PUT");
    await apiClient.patch("/v1/a", {});
    expect(lastCall(fetchSpy)[1].method).toBe("PATCH");
    await apiClient.delete("/v1/a");
    expect(lastCall(fetchSpy)[1].method).toBe("DELETE");
  });

  it("passes the abort signal through, so a screen that unmounts cancels its request", async () => {
    const fetchSpy = stubOkFetch();
    const controller = new AbortController();

    await apiClient.get("/v1/conversations", { signal: controller.signal });

    expect(lastCall(fetchSpy)[1].signal).toBe(controller.signal);
  });

  it("posts a raw body under its own media type without re-encoding it", async () => {
    const fetchSpy = stubOkFetch();
    setToken("mv-user-abc123");
    const jsonl = '{"schema_version":4}\n{"schema_version":4}\n';

    await apiClient.postRaw("/v1/imports", jsonl, "application/x-ndjson");

    const [, init] = lastCall(fetchSpy);
    const headers = init.headers as Record<string, string>;
    expect(headers["Content-Type"]).toBe("application/x-ndjson");
    expect(headers.Authorization).toBe("Bearer mv-user-abc123");
    expect(init.body).toBe(jsonl);
  });
});
