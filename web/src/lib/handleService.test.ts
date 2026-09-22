import { describe, expect, it } from "vitest";
import {
  formatHandleServiceLabel,
  HANDLE_SERVICE_OPTIONS,
  HANDLE_SERVICES,
  handleDuplicateKey,
  handlePlaceholder,
  handleValidationError,
  inferService,
} from "./handleService";

describe("handleService", () => {
  it("lists phone, email, whatsapp for profile/onboarding", () => {
    expect([...HANDLE_SERVICES]).toEqual(["phone", "email", "whatsapp"]);
    expect(HANDLE_SERVICE_OPTIONS.map((o) => o.value)).toEqual(["phone", "email", "whatsapp"]);
  });

  it("infers email and phone from handle when service empty", () => {
    expect(inferService("a@b.com", null)).toBe("email");
    expect(inferService("+1 (555) 123-4567", "")).toBe("phone");
    expect(inferService("alice", null)).toBe("unknown");
  });

  it("prefers an explicit service over inference", () => {
    expect(inferService("a@b.com", "WhatsApp")).toBe("whatsapp");
  });

  it("formats user-facing labels for the handles table", () => {
    expect(formatHandleServiceLabel("x", "imessage")).toBe("Text message");
    expect(formatHandleServiceLabel("x", "whatsapp")).toBe("WhatsApp");
    expect(formatHandleServiceLabel("a@b.com", null)).toBe("Email");
    expect(formatHandleServiceLabel("x", null)).toBe("—");
  });
});

describe("handlePlaceholder", () => {
  it("gives each service its own example", () => {
    expect(handlePlaceholder("phone")).toBe("+1 555-123-4567");
    expect(handlePlaceholder("email")).toBe("you@example.com");
    expect(handlePlaceholder("whatsapp")).toBe("+1 555-123-4567");
  });

  it("gives every option in the picker an example", () => {
    for (const option of HANDLE_SERVICE_OPTIONS) {
      expect(option.placeholder.length).toBeGreaterThan(0);
    }
  });

  it("calls a phone number what the contact drawer calls it", () => {
    expect(HANDLE_SERVICE_OPTIONS.find((o) => o.value === "phone")?.label).toBe("Text message");
  });
});

describe("handleValidationError", () => {
  it("passes an empty value, which is a row not filled in yet", () => {
    expect(handleValidationError("phone", "")).toBeNull();
    expect(handleValidationError("email", "   ")).toBeNull();
  });

  it("accepts the separators people actually type in a number", () => {
    expect(handleValidationError("phone", "+1 555-123-4567")).toBeNull();
    expect(handleValidationError("phone", "(555) 123.4567")).toBeNull();
    expect(handleValidationError("whatsapp", "+44 20 7946 0958")).toBeNull();
  });

  it("rejects a number that is not one", () => {
    expect(handleValidationError("phone", "notaphone")).toMatch(/phone number/);
    expect(handleValidationError("phone", "123")).toMatch(/phone number/);
    expect(handleValidationError("phone", "you@example.com")).toMatch(/phone number/);
  });

  it("rejects a number longer than E.164 allows", () => {
    expect(handleValidationError("phone", "+1234567890123456")).toMatch(/phone number/);
  });

  it("accepts an address and rejects what is not one", () => {
    expect(handleValidationError("email", "you@example.com")).toBeNull();
    expect(handleValidationError("email", "you@example")).toMatch(/email address/);
    expect(handleValidationError("email", "+1 555-123-4567")).toMatch(/email address/);
  });
});

describe("handleDuplicateKey", () => {
  it("has no key for a value with nothing to compare", () => {
    expect(handleDuplicateKey("phone", "")).toBeNull();
    expect(handleDuplicateKey("email", "   ")).toBeNull();
  });

  it("matches the same number however it was typed", () => {
    expect(handleDuplicateKey("phone", "+1 (555) 123-4567")).toBe(
      handleDuplicateKey("phone", "+15551234567"),
    );
  });

  it("matches an address regardless of case", () => {
    expect(handleDuplicateKey("email", "You@Example.com")).toBe(
      handleDuplicateKey("email", "you@example.com"),
    );
  });

  it("keeps the same number on two services apart, because that is two accounts", () => {
    expect(handleDuplicateKey("phone", "+15551234567")).not.toBe(
      handleDuplicateKey("whatsapp", "+15551234567"),
    );
  });

  it("does not fold a number written with and without its country code", () => {
    // Deliberate: guessing at country codes would let this refuse two numbers
    // that really are different.
    expect(handleDuplicateKey("phone", "+1 555-123-4567")).not.toBe(
      handleDuplicateKey("phone", "555-123-4567"),
    );
  });

  it("separates two different accounts", () => {
    expect(handleDuplicateKey("email", "a@example.com")).not.toBe(
      handleDuplicateKey("email", "b@example.com"),
    );
  });
});
