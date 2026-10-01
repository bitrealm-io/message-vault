/**
 * A random id in UUID version 4 form.
 *
 * Built on `crypto.getRandomValues` rather than `crypto.randomUUID`, because
 * browsers expose `randomUUID` only on HTTPS and localhost. A Message Crate
 * opened at `http://192.168.x.x:8080` has `getRandomValues` and nothing else.
 */
export function newId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  // Version 4 in the high bits of byte 6, the RFC 4122 variant in byte 8.
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
