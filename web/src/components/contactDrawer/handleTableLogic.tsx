import type { ReactNode } from "react";
import { formatHandleDate } from "./contactDrawerTypes";

export function handleDateCell(iso: string | null | undefined): string {
  return formatHandleDate(iso) ?? "—";
}

export function conversationCount(h: {
  individual_conversations: number;
  group_conversations: number;
}): number {
  return h.individual_conversations + h.group_conversations;
}

export type RemoveIdentityTarget = {
  handle: string;
  /** The service the vault recorded, or null when it recorded none. */
  service: string | null;
  serviceLabel: string;
  conversationCount: number;
};

export function removeIdentityConfirmBody(target: RemoveIdentityTarget): ReactNode {
  const { handle, serviceLabel, conversationCount } = target;
  const emphasize = "font-medium text-accent";
  const serviceId = (
    <>
      <span className={emphasize}>{serviceLabel}</span>{" "}
      <span className={`${emphasize} break-all`}>{handle}</span>
    </>
  );
  if (conversationCount <= 0) {
    return (
      <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
        Removing {serviceId} will unlink it from this contact.
      </p>
    );
  }
  const word = conversationCount === 1 ? "conversation" : "conversations";
  return (
    <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
      Removing {serviceId} will unlink {conversationCount} {word} from this contact. Unlinked data
      will not be deleted.
    </p>
  );
}
