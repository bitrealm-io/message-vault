import { useRef, useState } from "react";
import { apiErrorMessage } from "./apiErrorMessage";

/**
 * Select all on a paged list: load every page the list has not read yet,
 * then hand all of its rows to `onRows`, so an action that follows reaches
 * every row the list holds and not the page in hand (issue #1145).
 *
 * `scope` names what the list shows (its search, sort, group, and anything
 * else that changes its rows). An answer for a scope the list has since left
 * is dropped, and `selecting` and `error` belong to the scope they were
 * raised for, so a new search starts clear.
 */
export function useSelectAll<T>(
  loadAll: () => Promise<T[]>,
  scope: string,
  onRows: (rows: T[]) => void,
) {
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const run = useRef(0);
  const [selectingFor, setSelectingFor] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ scope: string; message: string } | null>(null);

  const selecting = selectingFor === scope;

  const selectAll = async () => {
    if (selecting) return;
    const mine = ++run.current;
    const forScope = scope;
    const current = () => mine === run.current && currentScope.current === forScope;
    setSelectingFor(forScope);
    setFailure(null);
    try {
      const rows = await loadAll();
      if (current()) onRows(rows);
    } catch (error) {
      if (current()) {
        setFailure({
          scope: forScope,
          message: `Select all could not read every row: ${apiErrorMessage(error, "the list did not load")}`,
        });
      }
    } finally {
      if (mine === run.current) setSelectingFor(null);
    }
  };

  /** Unticking the box drops a Select all still loading. */
  const cancel = () => {
    run.current += 1;
    setSelectingFor(null);
    setFailure(null);
  };

  return {
    selectAll,
    cancel,
    /** Pages are loading for Select all; the box waits. */
    selecting,
    /** Why the last Select all ticked nothing, for this scope. */
    error: failure?.scope === scope ? failure.message : null,
  };
}
