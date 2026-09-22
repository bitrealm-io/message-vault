import { type ContactSort, type ContactSortState, withSortField } from "../lib/contactSort";
import SortMenu, { type SortField } from "./SortMenu";

const FIELDS: ReadonlyArray<SortField<ContactSort>> = [
  { id: "first", label: "First Name" },
  { id: "last", label: "Last Name" },
  { id: "lastHeard", label: "Last Heard From" },
];

export default function ContactSortMenu({
  state,
  onChange,
}: {
  state: ContactSortState;
  onChange: (next: ContactSortState) => void;
}) {
  return (
    <SortMenu
      fields={FIELDS}
      sort={state.sort}
      order={state.order}
      onChange={(next) =>
        onChange(next.sort === state.sort ? next : withSortField(state, next.sort))
      }
      itemNoun="contacts"
    />
  );
}
