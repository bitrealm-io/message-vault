import { keepPreviousData } from "@tanstack/react-query";
import { useState } from "react";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import { listAccountAuditTrail, listAuditTrail } from "../../lib/serverApi";

/** Entries the Audit Trail shows per page. */
export const AUDIT_TRAIL_PAGE_SIZE = 50;

/**
 * Whose Audit Trail to read: every account's (the owner's view), the
 * logged-in account's own, or one account the owner has opened.
 */
export type AuditTrailOf = { kind: "all" } | { kind: "own" } | { kind: "account"; id: number };

/**
 * One page of an Audit Trail, newest first, and the page control's state.
 * The page on screen stays up while the next one loads, so the table does
 * not blank between pages. Changing whose trail is read starts at page one.
 */
export function useAuditTrail(of: AuditTrailOf) {
  const whose = of.kind === "account" ? of.id : of.kind;
  const [paging, setPaging] = useState({ whose, page: 0 });
  const page = paging.whose === whose ? paging.page : 0;
  const params = { limit: AUDIT_TRAIL_PAGE_SIZE, offset: page * AUDIT_TRAIL_PAGE_SIZE };

  const { data, isPending, error } = useRouteQuery(
    keys.auditTrail.page(whose, page),
    (signal) =>
      of.kind === "all"
        ? listAuditTrail(params, { signal })
        : listAccountAuditTrail(params, { signal }, of.kind === "account" ? of.id : undefined),
    { placeholderData: keepPreviousData },
  );

  return {
    entries: data?.items ?? [],
    total: data?.total ?? 0,
    page,
    setPage: (next: number) => setPaging({ whose, page: next }),
    loading: isPending,
    error: error ? apiErrorMessage(error, "Could not load the Audit Trail.") : "",
  };
}
