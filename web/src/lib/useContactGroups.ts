import { contactGroups } from "./contactGroups";
import { useNameCollection } from "./nameCollection";

/** Live list of contact groups for the logged-in account. */
export function useContactGroups(): {
  groups: string[];
  loading: boolean;
} {
  const { names, loading } = useNameCollection(contactGroups);
  return { groups: names, loading };
}
