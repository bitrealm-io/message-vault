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
import { formatIsoDateOnly } from "../lib/formatDate";
import { formatHandleServiceLabel } from "../lib/handleService";
import Button from "./Button";
import { TrashIcon } from "./icons";
import { type IdentityRow, identityTotals, sortIdentityRows } from "./identityRows";

export type { IdentityRow } from "./identityRows";

// One padding for every cell, so the columns line up under their headers.
const cellClass = "px-2 py-1.5 align-middle text-[0.813rem] leading-snug text-text";
const numberCellClass = `${cellClass} whitespace-nowrap text-right tabular-nums`;
const headerClass =
  "px-2 py-1.5 text-[0.688rem] font-semibold uppercase tracking-[0.04em] text-muted outline-none cursor-pointer hover:text-accent data-hovered:text-accent whitespace-nowrap";
const mutedClass = "text-muted";
const linkClass =
  "border-none bg-transparent p-0 text-[0.813rem] font-semibold leading-snug text-accent underline decoration-accent/80 underline-offset-2 cursor-pointer outline-none hover:decoration-accent focus-visible:rounded-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent";

function Dash() {
  return <span className={mutedClass}>—</span>;
}

function Count({
  value,
  loading,
  browseLabel,
  onBrowse,
}: {
  value: number;
  loading: boolean;
  browseLabel?: string;
  onBrowse?: () => void;
}) {
  if (loading || value === 0) return <Dash />;
  const text = value.toLocaleString();
  if (onBrowse) {
    return (
      <button type="button" className={linkClass} onClick={onBrowse} aria-label={browseLabel}>
        {text}
      </button>
    );
  }
  return <>{text}</>;
}

function DateCell({ value, loading }: { value: string | null; loading: boolean }) {
  const text = loading ? null : formatIsoDateOnly(value);
  return text ? <span className={mutedClass}>{text}</span> : <Dash />;
}

/** A header label with its sort arrow right after it; the arrow shows only while sorted. */
function Heading({
  children,
  sortDirection,
}: {
  children: ReactNode;
  sortDirection: "ascending" | "descending" | undefined;
}) {
  return (
    <>
      <span className={sortDirection ? "text-accent" : undefined}>{children}</span>
      <span
        aria-hidden="true"
        className={`ml-1 text-[0.55rem] leading-none ${sortDirection ? "text-accent" : "invisible"}`}
      >
        {sortDirection === "descending" ? "▼" : "▲"}
      </span>
    </>
  );
}

/**
 * The identities of one contact or one account, one row each, with what each
 * takes part in. Text columns sit left and numbers right, each header aligned
 * like its cells, and the browser lays the columns out: the Identity column
 * takes what is left, never less than a phone number's width, and cuts a
 * long address with an ellipsis. The caller gives the table a scrolling box
 * when its screen can be narrower than the eight columns.
 *
 * `onBrowse` turns a conversation count into a link (the contact drawer
 * passes one; the Profile tab does not), and `totals` adds the Summary row.
 */
export default function IdentityTable({
  rows,
  loading = false,
  busy = false,
  totals = false,
  emptyText = "No identities yet.",
  ariaLabel = "Identities",
  onRemove,
  onBrowse,
}: {
  rows: readonly IdentityRow[];
  /** The rows are placeholders until the vault answers: counts and dates show a dash. */
  loading?: boolean;
  /** A change is in flight, so Remove is disabled. */
  busy?: boolean;
  /** Add a Summary row with the earliest, the latest, and the sums. */
  totals?: boolean;
  emptyText?: string;
  ariaLabel?: string;
  onRemove: (row: IdentityRow) => void;
  /** Where a conversation count leads; with none, counts are plain numbers. */
  onBrowse?: (row: IdentityRow) => void;
}) {
  const [sort, setSort] = useState<SortDescriptor | null>(null);
  const sorted = useMemo(() => sortIdentityRows(rows, sort), [rows, sort]);
  const summary = useMemo(() => (totals ? identityTotals(rows) : null), [rows, totals]);

  if (rows.length === 0) {
    return <div className="text-[0.813rem] text-muted">{emptyText}</div>;
  }

  const renderCounts = (row: IdentityRow, browse: (() => void) | undefined) => (
    <>
      <Cell className={numberCellClass}>
        <DateCell value={row.start_date} loading={loading} />
      </Cell>
      <Cell className={numberCellClass}>
        <DateCell value={row.end_date} loading={loading} />
      </Cell>
      <Cell className={numberCellClass}>
        <Count
          value={row.conversations}
          loading={loading}
          browseLabel={`Open ${row.conversations.toLocaleString()} conversation${
            row.conversations === 1 ? "" : "s"
          }`}
          onBrowse={browse}
        />
      </Cell>
      <Cell className={numberCellClass}>
        <Count value={row.direct_messages} loading={loading} />
      </Cell>
      <Cell className={numberCellClass}>
        <Count value={row.group_messages} loading={loading} />
      </Cell>
    </>
  );

  return (
    <Table
      aria-label={ariaLabel}
      className="w-full border-collapse"
      sortDescriptor={sort ?? undefined}
      onSortChange={setSort}
    >
      <TableHeader className="border-b border-border">
        <Column id="service" allowsSorting className={`${headerClass} text-left`}>
          {({ sortDirection }) => <Heading sortDirection={sortDirection}>Service</Heading>}
        </Column>
        <Column id="address" isRowHeader allowsSorting className={`${headerClass} text-left`}>
          {({ sortDirection }) => <Heading sortDirection={sortDirection}>Identity</Heading>}
        </Column>
        <Column id="start_date" allowsSorting className={`${headerClass} text-right`}>
          {({ sortDirection }) => <Heading sortDirection={sortDirection}>First seen</Heading>}
        </Column>
        <Column id="end_date" allowsSorting className={`${headerClass} text-right`}>
          {({ sortDirection }) => <Heading sortDirection={sortDirection}>Last seen</Heading>}
        </Column>
        <Column id="conversations" allowsSorting className={`${headerClass} text-right`}>
          {({ sortDirection }) => <Heading sortDirection={sortDirection}>Conversations</Heading>}
        </Column>
        <Column id="direct_messages" allowsSorting className={`${headerClass} text-right`}>
          {({ sortDirection }) => (
            <Heading sortDirection={sortDirection}>
              Direct <span className="block">messages</span>
            </Heading>
          )}
        </Column>
        <Column id="group_messages" allowsSorting className={`${headerClass} text-right`}>
          {({ sortDirection }) => (
            <Heading sortDirection={sortDirection}>
              Group <span className="block">messages</span>
            </Heading>
          )}
        </Column>
        {/* Remove has no heading: each button is named after its identity. */}
        <Column className="w-9 px-1" aria-label="Actions">
          {""}
        </Column>
      </TableHeader>
      <TableBody>
        {sorted.map((row) => (
          <Row
            key={`${row.service ?? ""}-${row.address}`}
            id={`${row.service ?? ""}-${row.address}`}
            className="border-b border-border outline-none"
          >
            <Cell className={`${cellClass} whitespace-nowrap text-left text-muted`}>
              {formatHandleServiceLabel(row.address, row.service)}
            </Cell>
            <Cell className={`${cellClass} w-full min-w-[9rem] max-w-0 text-left`}>
              <span className="block truncate" title={row.address}>
                {row.address}
              </span>
            </Cell>
            {renderCounts(row, onBrowse ? () => onBrowse(row) : undefined)}
            <Cell className="px-1 py-0.5 text-right align-middle">
              <Button
                variant="ghostDanger"
                size="icon"
                isDisabled={busy || loading}
                aria-label={`Remove ${row.address} (${formatHandleServiceLabel(row.address, row.service)})`}
                title="Remove identity"
                onPress={() => onRemove(row)}
              >
                <TrashIcon />
              </Button>
            </Cell>
          </Row>
        ))}
        {summary ? (
          <Row id="summary" className="border-t-2 border-border font-semibold outline-none">
            <Cell className={`${cellClass} text-left`}>Summary</Cell>
            <Cell className={`${cellClass} text-left text-muted`}>—</Cell>
            {renderCounts(summary, undefined)}
            <Cell className="px-1">{""}</Cell>
          </Row>
        ) : null}
      </TableBody>
    </Table>
  );
}
