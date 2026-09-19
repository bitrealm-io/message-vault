/**
 * Shift + click on a list checkbox, the way Gmail does it. Every row from the
 * last clicked checkbox (`anchorId`) to the clicked one, both ends included,
 * takes the state the clicked box lands in (`on`). So a range is checked by
 * clicking one end and shift-clicking the other, and unchecked the same way.
 *
 * `orderedIds` is the list as shown. With no anchor, or an anchor that is no
 * longer in the list, only the clicked row changes. Checked ids missing from
 * the list are kept.
 */
export function applyCheckedRange(
  orderedIds: readonly string[],
  checked: ReadonlySet<string>,
  anchorId: string | null,
  clickedId: string,
  on: boolean,
): Set<string> {
  const next = new Set(checked);
  const clicked = orderedIds.indexOf(clickedId);
  if (clicked === -1) return next;
  const anchor = anchorId === null ? -1 : orderedIds.indexOf(anchorId);
  const from = anchor === -1 ? clicked : Math.min(anchor, clicked);
  const to = anchor === -1 ? clicked : Math.max(anchor, clicked);
  for (let i = from; i <= to; i++) {
    if (on) next.add(orderedIds[i]);
    else next.delete(orderedIds[i]);
  }
  return next;
}
