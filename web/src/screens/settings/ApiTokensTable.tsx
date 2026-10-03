import { Cell, Column, Row, Table, TableBody, TableHeader } from "react-aria-components";
import Button from "../../components/Button";
import { PencilIcon, TrashIcon } from "../../components/icons";
import type { ApiTokenItem } from "./apiTokensUtils";
import {
  displayTokenHint,
  formatTokenDate,
  permissionsLabel,
  tdClass,
  tdMuted,
  thClass,
} from "./apiTokensUtils";

export default function ApiTokensTable({
  items,
  busy,
  composing,
  onRename,
  onRevoke,
}: {
  items: ApiTokenItem[];
  busy: boolean;
  composing: boolean;
  onRename: (item: ApiTokenItem) => void;
  onRevoke: (item: ApiTokenItem) => void;
}) {
  return (
    <div className="overflow-hidden rounded-xl border border-border bg-elevated">
      <Table
        aria-label="API Tokens"
        selectionMode="none"
        className="w-full table-fixed border-collapse text-left outline-none"
      >
        <TableHeader className="border-b border-border">
          <Column isRowHeader className={`${thClass} w-[18%]`}>
            Name
          </Column>
          <Column className={`${thClass} w-[18%]`}>Token</Column>
          <Column className={`${thClass} w-[19%]`}>Permissions</Column>
          <Column className={`${thClass} w-[12%]`}>Created</Column>
          <Column className={`${thClass} w-[13%]`}>Last Used</Column>
          <Column className={`${thClass} w-[12%]`}>Expires</Column>
          <Column className={`${thClass} w-[8%]`} />
        </TableHeader>
        <TableBody
          items={items}
          dependencies={[busy]}
          renderEmptyState={() =>
            composing ? null : (
              <div className="px-5 py-6 text-[0.75rem] text-muted">No API Tokens yet.</div>
            )
          }
          className="outline-none"
        >
          {(item) => (
            <Row
              id={item.id}
              className="border-b border-border last:border-b-0 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
            >
              <Cell className={`${tdClass} truncate font-medium`}>
                <span className="block truncate" title={item.label}>
                  {item.label}
                </span>
              </Cell>
              <Cell className={`${tdMuted} truncate font-mono text-[0.688rem]`}>
                <span className="block truncate" title="Masked API Token">
                  {displayTokenHint(item.token_hint)}
                </span>
              </Cell>
              <Cell className={tdClass}>{permissionsLabel(item)}</Cell>
              <Cell className={tdMuted}>{formatTokenDate(item.created_at)}</Cell>
              <Cell className={tdMuted}>{formatTokenDate(item.last_accessed_at)}</Cell>
              <Cell className={tdMuted}>{formatTokenDate(item.expires_at)}</Cell>
              <Cell className={`${tdClass}`}>
                <div className="flex items-center justify-end gap-1">
                  <Button
                    variant="ghostNeutral"
                    size="icon"
                    disabled={busy}
                    title="Edit API Token"
                    aria-label="Edit API Token"
                    onClick={() => onRename(item)}
                  >
                    <PencilIcon />
                  </Button>
                  <Button
                    variant="ghostDanger"
                    size="icon"
                    disabled={busy}
                    title="Revoke API Token"
                    aria-label="Revoke API Token"
                    onClick={() => onRevoke(item)}
                  >
                    <TrashIcon />
                  </Button>
                </div>
              </Cell>
            </Row>
          )}
        </TableBody>
      </Table>
    </div>
  );
}
