import Button from "../../../components/Button";

/**
 * "Page X of N" with Back and Next, under a table that shows one page of a
 * longer list. Nothing is drawn while the list fits on one page.
 */
export default function PageControl({
  page,
  total,
  pageSize,
  onPageChange,
}: {
  /** The page on screen, counted from 0. */
  page: number;
  /** Rows in the whole list, across every page. */
  total: number;
  pageSize: number;
  onPageChange: (page: number) => void;
}) {
  if (total <= pageSize) return null;
  const pageCount = Math.ceil(total / pageSize);

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-[0.75rem] text-muted">
        Page {page + 1} of {pageCount}
      </span>
      <div className="flex gap-2">
        <Button disabled={page <= 0} onClick={() => onPageChange(Math.max(0, page - 1))} size="sm">
          Back
        </Button>
        <Button
          disabled={page >= pageCount - 1}
          onClick={() => onPageChange(Math.min(pageCount - 1, page + 1))}
          size="sm"
        >
          Next
        </Button>
      </div>
    </div>
  );
}
