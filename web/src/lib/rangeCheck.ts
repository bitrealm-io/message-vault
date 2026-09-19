/**
 * Shift + click on a list checkbox. Checks every row from the furthest checked
 * row to the clicked one, both ends included: a click above the topmost checked
 * row reaches down to the bottommost, a click below the bottommost reaches up
 * to the topmost, and a click between them fills the gaps. It only ever checks;
 * nothing is unchecked. With no row checked it checks the clicked row alone.
 *
 * `orderedIds` is the list as shown. Checked ids missing from it are kept.
 */
export function extendCheckedRange(
  orderedIds: readonly string[],
  checked: ReadonlySet<string>,
  clickedId: string,
): Set<string> {
  const next = new Set(checked);
  const clicked = orderedIds.indexOf(clickedId);
  if (clicked === -1) return next;
  let from = clicked;
  let to = clicked;
  orderedIds.forEach((id, i) => {
    if (!checked.has(id)) return;
    if (i < from) from = i;
    if (i > to) to = i;
  });
  for (let i = from; i <= to; i++) next.add(orderedIds[i]);
  return next;
}
