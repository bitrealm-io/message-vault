import { afterEach, describe, expect, it, vi } from "vitest";
import { apiClient, problemFromBody, setBaseUrl, VaultApiError } from "./api";

afterEach(() => {
  vi.unstubAllGlobals();
  setBaseUrl("");
});

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

    await expect(apiClient.post("/v1/auth/register", {})).rejects.toMatchObject({
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
