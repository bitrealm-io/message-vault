import { IMESSAGE_SOURCE_ID, isImessageMethod } from "./imessageImport";
import { isWhatsappMethod, WHATSAPP_SOURCE_ID } from "./whatsappImport";

/** Import session / messages.source slug for a desktop Import method id. */
export function sourceForMethod(source: string): string {
  if (isImessageMethod(source)) {
    return IMESSAGE_SOURCE_ID;
  }
  if (isWhatsappMethod(source)) {
    return WHATSAPP_SOURCE_ID;
  }
  return source;
}

/** Body for POST /v1/imports. Maps method ids; leaves other sources as-is. */
export function importSessionCreateBody(formSource: string): {
  source: string;
  tool: "message-crate";
  mode: "append";
} {
  return {
    source: sourceForMethod(formSource),
    tool: "message-crate",
    mode: "append",
  };
}
