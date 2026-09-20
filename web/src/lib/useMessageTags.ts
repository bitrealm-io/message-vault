import { messageTags } from "./messageTags";
import { useNameCollection } from "./nameCollection";

/** Live list of message tags for the logged-in account. */
export function useMessageTags(): {
  tags: string[];
  loading: boolean;
} {
  const { names, loading } = useNameCollection(messageTags);
  return { tags: names, loading };
}
