import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  Cell,
  Column,
  Row,
  type SortDescriptor,
  Table,
  TableBody,
  TableHeader,
} from "react-aria-components";
import type { ContactDetail } from "../lib/contactDetail";
import { contactLabelText } from "../lib/contactLabel";
import { getContactSummaries } from "../lib/vaultApi";
import { keys } from "../lib/vaultKeys";
import { useVaultCache } from "../lib/vaultQuery";
import Button from "./Button";
import ContactLabel from "./ContactLabel";
import { type ContactPreview, sumHandleTotals } from "./contactDrawer/contactDrawerTypes";
import { CountCell, SortableColumn } from "./contactDrawer/handleTableHelpers";
import { conversationCount, handleDateCell } from "./contactDrawer/handleTableLogic";
import {
  mutedClass,
  tdCenterClass,
  tdClass,
  tdRightClass,
  thClass,
} from "./contactDrawer/handleTableStyles";
import DataCard, { dataCardHeaderRowClass } from "./DataCard";

type ContactTotals = ReturnType<typeof sumHandleTotals>;

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

type ContactRow = {
  id: string;
  name: string;
  handles: string[] | undefined;
  totals: ContactTotals | null;
};

function totalsFromSummary(summary: ContactSelectionSummary): ContactTotals {
  return {
    individual_conversations: summary.individual_conversations,
    group_conversations: summary.group_conversations,
    individual_message_count: summary.individual_message_count,
    group_message_count: summary.group_message_count,
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
      return contactLabelText(row.name, row.handles).toLowerCase();
    case "start_date":
      return totals?.start_date ?? "";
    case "end_date":
      return totals?.end_date ?? "";
    case "conversations":
      return totals ? conversationCount(totals) : -1;
    case "direct_messages":
      return totals?.individual_message_count ?? -1;
    case "group_messages":
      return totals?.group_message_count ?? -1;
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
  const cache = useVaultCache();
  const heading =
    contacts.length === 1 ? "1 contact selected" : `${contacts.length} contacts selected`;
  const [metrics, setMetrics] = useState<Record<string, RowMetrics>>({});
  const [sortDescriptor, setSortDescriptor] = useState<SortDescriptor | null>(null);
  const contactKey = contacts.map((c) => c.id).join(",");
  const contactsRef = useRef(contacts);
  contactsRef.current = contacts;

  useEffect(() => {
    void contactKey;
    const selected = contactsRef.current;
    const ac = new AbortController();
    const seeded: Record<string, RowMetrics> = {};
    const missing: string[] = [];
    for (const c of selected) {
      const cached = cache.read<ContactDetail>(keys.contacts.detail(c.id));
      if (cached) {
        seeded[c.id] = {
          name: cached.name,
          totals: sumHandleTotals(cached.handles),
        };
      } else {
        missing.push(c.id);
      }
    }
    setMetrics(seeded);
    const batches = chunkIds(missing, SUMMARY_BATCH_SIZE)
      .map((ids) => ids.map(Number).filter((id) => Number.isFinite(id) && id > 0))
      .filter((ids) => ids.length > 0);
    if (batches.length === 0) {
      return () => ac.abort();
    }
    void Promise.all(batches.map((ids) => getContactSummaries({ ids }, { signal: ac.signal })))
      .then((pages) => {
        if (ac.signal.aborted) return;
        setMetrics((prev) => {
          const next = { ...prev };
          for (const page of pages) {
            for (const summary of page.items) {
              next[String(summary.id)] = {
                name: summary.name,
                totals: totalsFromSummary(summary),
              };
            }
          }
          return next;
        });
      })
      .catch(() => {
        /* aborted or failed — uncached rows stay on em dash until a later load */
      });
    return () => ac.abort();
  }, [contactKey, cache.read]);

  const rows = useMemo<ContactRow[]>(() => {
    const built = contacts.map((c) => {
      const row = metrics[c.id];
      return {
        id: c.id,
        name: row?.name ?? c.name,
        handles: c.handles,
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
      return contactLabelText(a.name, a.handles).localeCompare(contactLabelText(b.name, b.handles));
    });
  }, [contacts, metrics, sortDescriptor]);

  return (
    <aside
      className="flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden overflow-y-auto bg-panel px-6 pb-6 pt-2 outline-none [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      aria-label={heading}
    >
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
              First Seen
            </SortableColumn>
            <SortableColumn id="end_date" widthClass="w-[14%]">
              Last Seen
            </SortableColumn>
            <SortableColumn id="conversations" widthClass="w-[14%]" align="right">
              Threads
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
                    <ContactLabel name={row.name} handles={row.handles} />
                  </span>
                </Cell>
                <Cell className={`${tdCenterClass} whitespace-nowrap text-muted`}>
                  <MetricCell loaded={row.totals != null}>
                    {handleDateCell(row.totals?.start_date)}
                  </MetricCell>
                </Cell>
                <Cell className={`${tdCenterClass} whitespace-nowrap text-muted`}>
                  <MetricCell loaded={row.totals != null}>
                    {handleDateCell(row.totals?.end_date)}
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals ? conversationCount(row.totals) : 0} />
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals?.individual_message_count ?? 0} />
                  </MetricCell>
                </Cell>
                <Cell className={tdRightClass}>
                  <MetricCell loaded={row.totals != null}>
                    <CountCell value={row.totals?.group_message_count ?? 0} />
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
