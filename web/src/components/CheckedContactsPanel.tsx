import { type ReactNode, useMemo, useState } from "react";
import {
  Cell,
  Column,
  Row,
  type SortDescriptor,
  Table,
  TableBody,
  TableHeader,
} from "react-aria-components";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import type { ContactDetail } from "../lib/contactDetail";
import { contactLabelText } from "../lib/contactLabel";
import { keys } from "../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../lib/routeQuery";
import { getContactSummaries } from "../lib/serverApi";
import { useTimeZone } from "../lib/timeZone";
import Button from "./Button";
import ContactLabel from "./ContactLabel";
import { type ContactPreview, contactTotals } from "./contactDrawer/contactDrawerTypes";
import { CountCell, SortableColumn } from "./contactDrawer/handleTableHelpers";
import { handleDateCell } from "./contactDrawer/handleTableLogic";
import {
  mutedClass,
  tdCenterClass,
  tdClass,
  tdRightClass,
  thClass,
} from "./contactDrawer/handleTableStyles";
import DataCard, { dataCardHeaderRowClass } from "./DataCard";

type ContactTotals = ReturnType<typeof contactTotals>;

/** Matches `MAX_CONTACT_SUMMARY_IDS` on `POST /v1/contacts/summaries`. */
const SUMMARY_BATCH_SIZE = 500;

type ContactSelectionSummary = {
  id: string | number;
  name: string;
  start_date?: string | null;
  end_date?: string | null;
  individual_conversations: number;
  group_conversations: number;
  individual_message_count: number;
  group_message_count: number;
};

type RowMetrics = {
  name: string;
  totals: ContactTotals;
};

/**
 * Figures for every id, asked for in batches the server accepts.
 *
 * Ids that are not positive numbers cannot name a stored contact, so they are
 * not sent; their rows keep "—".
 */
async function fetchSummaries(
  ids: readonly string[],
  signal: AbortSignal,
): Promise<Record<string, RowMetrics>> {
  const batches = chunkIds([...ids], SUMMARY_BATCH_SIZE)
    .map((batch) => batch.map(Number).filter((id) => Number.isFinite(id) && id > 0))
    .filter((batch) => batch.length > 0);
  const pages = await Promise.all(
    batches.map((batch) => getContactSummaries({ ids: batch }, { signal })),
  );
  const metrics: Record<string, RowMetrics> = {};
  for (const page of pages) {
    for (const summary of page.items) {
      metrics[String(summary.id)] = {
        name: summary.name,
        totals: totalsFromSummary(summary),
      };
    }
  }
  return metrics;
}

type ContactRow = {
  id: string;
  name: string;
  addresses: string[] | undefined;
  totals: ContactTotals | null;
};

function totalsFromSummary(summary: ContactSelectionSummary): ContactTotals {
  return {
    conversations: summary.individual_conversations + summary.group_conversations,
    direct_messages: summary.individual_message_count,
    group_messages: summary.group_message_count,
    start_date: summary.start_date ?? null,
    end_date: summary.end_date ?? null,
  };
}

function chunkIds(ids: string[], size: number): string[][] {
  const chunks: string[][] = [];
  for (let i = 0; i < ids.length; i += size) {
    chunks.push(ids.slice(i, i + size));
  }
  return chunks;
}

function sortValue(row: ContactRow, col: string): string | number {
  const totals = row.totals;
  switch (col) {
    case "name":
      return contactLabelText(row.name, row.addresses).toLowerCase();
    case "start_date":
      return totals?.start_date ?? "";
    case "end_date":
      return totals?.end_date ?? "";
    case "conversations":
      return totals?.conversations ?? -1;
    case "direct_messages":
      return totals?.direct_messages ?? -1;
    case "group_messages":
      return totals?.group_messages ?? -1;
    default:
      return "";
  }
}

function MetricCell({ loaded, children }: { loaded: boolean; children: ReactNode }) {
  if (!loaded) {
    return <span className={mutedClass}>—</span>;
  }
  return children;
}

/** Right-hand card of contacts whose checkboxes are on, with identity-table totals. */
export default function CheckedContactsPanel({
  contacts,
  onClear,
}: {
  contacts: ContactPreview[];
  onClear: () => void;
}) {
  const cache = useRouteCache();
  const zone = useTimeZone();
  const heading =
    contacts.length === 1 ? "1 contact selected" : `${contacts.length} contacts selected`;
  const [sortDescriptor, setSortDescriptor] = useState<SortDescriptor | null>(null);
  const ids = useMemo(() => contacts.map((c) => c.id), [contacts]);
  // The figures come from `POST /v1/contacts/summaries`. A contact whose
  // drawer was opened in this session already has its own figures in the
  // cache, so its row shows those while the summaries load, or if they fail.
  const summaries = useRouteQuery(
    keys.contacts.summaries(ids),
    (signal) => fetchSummaries(ids, signal),
    { enabled: ids.length > 0 },
  );

  const metrics = useMemo<Record<string, RowMetrics>>(() => {
    const merged: Record<string, RowMetrics> = {};
    for (const id of ids) {
      const cached = cache.read<ContactDetail>(keys.contacts.detail(id));
      if (cached) {
        merged[id] = { name: cached.name, totals: contactTotals(cached) };
      }
    }
    return { ...merged, ...summaries.data };
  }, [ids, cache, summaries.data]);

  const rows = useMemo<ContactRow[]>(() => {
    const built = contacts.map((c) => {
      const row = metrics[c.id];
      return {
        id: c.id,
        name: row?.name ?? c.name,
        addresses: c.addresses,
        totals: row?.totals ?? null,
      };
    });
    if (!sortDescriptor?.column) return built;
    const col = String(sortDescriptor.column);
    const dir = sortDescriptor.direction === "descending" ? -1 : 1;
    return [...built].sort((a, b) => {
      const av = sortValue(a, col);
      const bv = sortValue(b, col);
      if (av < bv) return -1 * dir;
      if (av > bv) return 1 * dir;
      return contactLabelText(a.name, a.addresses).localeCompare(
        contactLabelText(b.name, b.addresses),
      );
    });
  }, [contacts, metrics, sortDescriptor]);

  return (
    <aside
      className="flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden overflow-y-auto bg-panel px-6 pb-6 pt-2 outline-none [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      aria-label={heading}
    >
      {summaries.error && !summaries.data ? (
        <div
          role="alert"
          className="mb-3 flex items-start justify-between gap-3 rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.813rem] text-danger"
        >
          <div className="min-w-0">
            <p className="m-0 font-semibold">The figures for these contacts could not be loaded.</p>
            <p className="m-0 mt-1">
              {apiErrorMessage(summaries.error, "The server did not answer.")}
            </p>
          </div>
          <Button variant="secondary" size="chip" onClick={() => void summaries.refetch()}>
            Try again
          </Button>
        </div>
      ) : null}
      <DataCard
        title={<h2 className="m-0 text-[1.125rem] font-semibold">{heading}</h2>}
        toolbar={
          <Button variant="secondary" onClick={onClear} size="chip">
            Clear contacts
          </Button>
        }
        bodyClassName="overflow-x-hidden"
      >
        <Table
          aria-label={heading}
          className="w-full border-collapse text-left table-fixed"
          sortDescriptor={sortDescriptor ?? undefined}
          onSortChange={setSortDescriptor}
        >
          <TableHeader className={dataCardHeaderRowClass}>
            <Column id="name" isRowHeader allowsSorting className={`${thClass} w-[28%] !text-left`}>
              {({ sortDirection }) => (
                <span className="relative inline-flex items-center justify-start">
                  <span className="text-left leading-tight">Contact</span>
                  <span
                    aria-hidden="true"
                    className={`absolute top-1/2 left-[calc(100%+0.25rem)] -translate-y-1/2 text-[0.55rem] leading-none ${
                      sortDirection ? "text-accent" : "invisible"
                    }`}
                  >
                    {sortDirection === "descending" ? "▼" : "▲"}
                  </span>
                </span>
              )}
            </Column>
            <SortableColumn id="start_date" widthClass="w-[14%]">
              First heard from
            </SortableColumn>
            <SortableColumn id="end_date" widthClass="w-[14%]">
              Last heard from
            </SortableColumn>
            <SortableColumn id="conversations" widthClass="w-[14%]" align="right">
              Conversations
            </SortableColumn>
            <SortableColumn id="direct_messages" widthClass="w-[15%]" align="right">
              Direct
              <br />
              Messages
            </SortableColumn>
            <SortableColumn id="group_messages" widthClass="w-[15%]" align="right">
              Group
              <br />
              Messages
            </SortableColumn>
          </TableHeader>
          <TableBody
            items={rows}
            dependencies={[sortDescriptor, metrics]}
            className="[&_tr]:border-b [&_tr]:border-border"
          >
            {(row) => (
              <Row id={row.id} className="outline-none">
                <Cell className={`${tdClass} !text-left`}>
                  <span className="min-w-0 truncate font-medium">
                    <ContactLabel name={row.name} addresses={row.addresses} />
                  </span>
                </Cell>
                <Cell className={`${tdCenterClass} whitespace-nowrap text-muted`}>
                  <MetricCell loaded={row.totals != null}>
                    {handleDateCell(row.totals?.start_date, zone)}
                  </MetricCell>
                </Cell>
                <Cell className={`${tdCenterClass} whitespace-nowrap text-muted`}>
                  <MetricCell loaded={row.totals != null}>
                    {handleDateCell(row.totals?.end_date, zone)}
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals?.conversations ?? 0} />
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals?.direct_messages ?? 0} />
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals?.group_messages ?? 0} />
                  </MetricCell>
                </Cell>
              </Row>
            )}
          </TableBody>
        </Table>
      </DataCard>
    </aside>
  );
}
