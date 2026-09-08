import { type InfiniteData, type UseMutationResult, useMutation } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import {
  type OffsetPage,
  useVaultCache,
  useVaultQuery,
  type VaultCacheEntries,
  type VaultQueryKey,
} from "./vaultQuery";

/**
 * Contact Groups and Message Tags are the same feature over different nouns: a
 * named set the account owns, and a membership that puts rows in or out of it.
 * This builds one from a description of the nouns so the two do not drift
 * apart.
 *
 * The vault addresses a set by its id; screens, the sidebar, and the router
 * hold names. The lookup from one to the other lives here and nowhere else:
 * the id comes from the cached list, or from the vault once when the cached
 * list does not hold the name, and a name the vault does not know is an error
 * before any request is sent. See `docs/agents/http-api-rules.md`, Identifiers.
 */

/** One set as the vault answers it. */
export type NamedSet = { id: number; name: string };

/** Members to put in and take out of a set, in one request. Either side may be left off. */
export type MembersPatch = { add?: number[]; remove?: number[] };

/** What a membership write answers with. */
export type MembersChanged = { added: number; removed: number };

/** A name to put on or take off some rows, as a screen asks for it. */
export type SetMembersVars = { name: string; patch: MembersPatch };

/**
 * A cached shape whose rows carry this collection's names as chips.
 *
 * A membership write patches these before the vault answers, so a ticked box
 * shows on a long list without a round trip. The collection describes them;
 * the screen does not, which is why no screen keeps an override map.
 */
export type ChipTarget = {
  /** Prefix of the entries to patch. */
  key: VaultQueryKey;
  /** Field the names sit in on a row. */
  field: "groups" | "tags";
  /** `pages` for an offset-paged list entry, `row` for one row on its own. */
  shape: "pages" | "row";
};

/** The vault calls one of these collections is built from. */
export type NameCollectionRoutes = {
  list: (opts?: { signal?: AbortSignal }) => Promise<{ items: NamedSet[] }>;
  create: (body: { name: string }) => Promise<NamedSet>;
  update: (id: number, body: { name: string }) => Promise<NamedSet>;
  remove: (id: number) => Promise<void>;
  updateMembers: (
    id: number,
    body: { add: number[]; remove: number[] },
  ) => Promise<{ added: number; removed: number }>;
};

export type NameCollectionConfig = {
  routes: NameCollectionRoutes;
  /** This collection's cache prefix, from `vaultKeys`. */
  key: VaultQueryKey;
  /**
   * Cache keys of the lists that show these names as chips, invalidated after
   * every write. Matched by prefix, so `keys.contacts.all` covers every page
   * and every search of the contact list.
   */
  invalidates: readonly VaultQueryKey[];
  /** Cached shapes to patch with this collection's names before the vault answers. */
  chips: readonly ChipTarget[];
  /** What one of these is called in an error, e.g. `group`. */
  label: string;
  /** This collection's search term for one name, e.g. `forGroup` from `searchQuery.ts`. */
  forName: (name: string) => string;
  reservedNames: ReadonlySet<string>;
  reservedError: (name: string) => string;
};

export type NameCollection = {
  /** Cache key parts, before the account is put in front of them. */
  key: VaultQueryKey;
  routes: NameCollectionRoutes;
  invalidates: readonly VaultQueryKey[];
  chips: readonly ChipTarget[];
  label: string;
  isReserved: (name: string) => boolean;
  reservedError: (name: string) => string;
  /** Build the list query for one page of this collection plus a typed search. */
  listQuery: (name: string | "none" | null, search: string) => string;
};

export function createNameCollection(config: NameCollectionConfig): NameCollection {
  const isReserved = (name: string) => config.reservedNames.has(name.trim().toLowerCase());

  function listQuery(name: string | "none" | null, search: string): string {
    const parts: string[] = [];
    if (name) parts.push(config.forName(name));
    const extra = search.trim();
    if (extra) parts.push(extra);
    return parts.join(" ");
  }

  return {
    key: config.key,
    routes: config.routes,
    invalidates: config.invalidates,
    chips: config.chips,
    label: config.label,
    isReserved,
    reservedError: config.reservedError,
    listQuery,
  };
}

/** The cache holds the vault's list as it came, ids included. */
async function fetchSets(collection: NameCollection, signal: AbortSignal): Promise<NamedSet[]> {
  return (await collection.routes.list({ signal })).items;
}

/** One row of a list that shows names as chips. */
type ChipRow = { id: string | number } & Record<string, unknown>;

/**
 * Add or remove one name, matching letter case the way the lists match it.
 *
 * `Family` and `family` are one name to a person, so ticking the box when a
 * row already has the name under another spelling changes nothing.
 */
export function withName(names: readonly string[], name: string, enable: boolean): string[] {
  const has = names.some((n) => n.toLowerCase() === name.toLowerCase());
  if (enable) return has ? [...names] : [...names, name];
  return names.filter((n) => n.toLowerCase() !== name.toLowerCase());
}

/** Rewrite one cache entry so the rows named by `ids` gain or lose the name. */
export function patchChips(
  entry: unknown,
  target: ChipTarget,
  ids: ReadonlySet<string>,
  name: string,
  enable: boolean,
): unknown {
  if (!entry || ids.size === 0) return entry;
  const patchRow = (row: ChipRow): ChipRow => {
    if (!ids.has(String(row.id))) return row;
    const current = row[target.field];
    return {
      ...row,
      [target.field]: withName(Array.isArray(current) ? (current as string[]) : [], name, enable),
    };
  };
  if (target.shape === "row") return patchRow(entry as ChipRow);
  const paged = entry as InfiniteData<OffsetPage<ChipRow>>;
  if (!Array.isArray(paged.pages)) return entry;
  return {
    ...paged,
    pages: paged.pages.map((page) => ({ ...page, items: page.items.map(patchRow) })),
  };
}

/** Live list of one collection's names for the signed-in account. */
export function useNameCollection(collection: NameCollection): {
  names: string[];
  loading: boolean;
} {
  const { data, isPending } = useVaultQuery(collection.key, (signal) =>
    fetchSets(collection, signal),
  );
  const names = useMemo(() => (data ?? []).map((set) => set.name), [data]);
  return { names, loading: isPending };
}

/**
 * The id behind a name: from the cache, else from the vault once, else an
 * error and no request. The vault-once path covers creating a set and adding
 * to it before the invalidated list has come back.
 */
function useIdOf(collection: NameCollection): (name: string) => Promise<number> {
  const cache = useVaultCache();
  return useCallback(
    async (name: string) => {
      const wanted = name.trim().toLowerCase();
      const find = (sets: NamedSet[] | undefined) =>
        sets?.find((set) => set.name.toLowerCase() === wanted)?.id;
      const hit = find(cache.read<NamedSet[]>(collection.key));
      if (hit !== undefined) return hit;
      const fresh = find(
        await cache.fetch<NamedSet[]>(collection.key, (signal) => fetchSets(collection, signal)),
      );
      if (fresh !== undefined) return fresh;
      throw new Error(`${collection.label} not found`);
    },
    [cache, collection],
  );
}

/** A name nobody may use, refused before any request is sent. */
function checkedName(collection: NameCollection, name: string): string {
  const trimmed = name.trim();
  if (!trimmed) throw new Error("name required");
  if (collection.isReserved(trimmed)) throw new Error(collection.reservedError(trimmed));
  return trimmed;
}

/** This collection's list, plus every list that shows its names as chips. */
function useMarkStale(collection: NameCollection): () => Promise<void> {
  const cache = useVaultCache();
  return useCallback(async () => {
    await cache.invalidate(collection.key, ...collection.invalidates);
  }, [cache, collection]);
}

export function useCreateNamedSet(
  collection: NameCollection,
): UseMutationResult<NamedSet, Error, string> {
  const markStale = useMarkStale(collection);
  return useMutation<NamedSet, Error, string>({
    mutationFn: async (name) => collection.routes.create({ name: checkedName(collection, name) }),
    onSettled: markStale,
  });
}

export function useRenameNamedSet(
  collection: NameCollection,
): UseMutationResult<NamedSet, Error, { from: string; to: string }> {
  const idOf = useIdOf(collection);
  const markStale = useMarkStale(collection);
  return useMutation<NamedSet, Error, { from: string; to: string }>({
    mutationFn: async ({ from, to }) => {
      const name = checkedName(collection, to);
      return collection.routes.update(await idOf(from), { name });
    },
    onSettled: markStale,
  });
}

export function useDeleteNamedSet(
  collection: NameCollection,
): UseMutationResult<void, Error, string> {
  const idOf = useIdOf(collection);
  const markStale = useMarkStale(collection);
  return useMutation<void, Error, string>({
    mutationFn: async (name) => collection.routes.remove(await idOf(name)),
    onSettled: markStale,
  });
}

/** The rows as they were before an optimistic membership write touched them. */
export type ChipSnapshot = { entries: VaultCacheEntries };

/**
 * Put rows in or out of one set, drawn before the vault answers.
 *
 * The chips change on the list and on the open contact at once, and every
 * list showing the name is marked stale once it settles. Two of these can be
 * in flight together — the Clear all button fires one per name — but the
 * rollback is a whole-entry snapshot: if the earlier of two overlapping
 * writes fails, restoring its snapshot overwrites the later one's optimistic
 * chips too, until the `onSettled` invalidation refetches and the two
 * converge on what the vault actually has.
 */
export function useSetNamedSetMembers(
  collection: NameCollection,
): UseMutationResult<MembersChanged, Error, SetMembersVars, ChipSnapshot> {
  const cache = useVaultCache();
  const idOf = useIdOf(collection);
  const markStale = useMarkStale(collection);
  return useMutation<MembersChanged, Error, SetMembersVars, ChipSnapshot>({
    mutationFn: async ({ name, patch }) =>
      collection.routes.updateMembers(await idOf(name), {
        add: patch.add ?? [],
        remove: patch.remove ?? [],
      }),
    onMutate: async ({ name, patch }) => {
      const add = new Set((patch.add ?? []).map(String));
      const remove = new Set((patch.remove ?? []).map(String));
      for (const target of collection.chips) await cache.cancel(target.key);
      const entries = collection.chips.flatMap((target) => cache.snapshot(target.key));
      for (const target of collection.chips) {
        cache.patch<unknown>(target.key, (entry) =>
          patchChips(patchChips(entry, target, add, name, true), target, remove, name, false),
        );
      }
      return { entries };
    },
    onError: (_error, _vars, context) => {
      if (context) cache.restore(context.entries);
    },
    onSettled: markStale,
  });
}

/** What a screen or the sidebar does to one of these collections. */
export type NameCollectionActions = {
  create: (name: string) => Promise<string>;
  rename: (from: string, to: string) => Promise<string>;
  remove: (name: string) => Promise<void>;
  setMembers: (name: string, patch: MembersPatch) => Promise<MembersChanged>;
  invalidate: () => Promise<void>;
  /** Any of the four in flight, so a screen needs no busy flag of its own. */
  pending: boolean;
  /** The newest of the four to fail, or null once a later one succeeds. */
  error: Error | null;
};

/**
 * The four writes, behind names.
 *
 * Screens keep passing names; the ids, the optimistic chips, the rollback and
 * the invalidation all belong to the mutations above.
 */
export function useNameCollectionActions(collection: NameCollection): NameCollectionActions {
  const cache = useVaultCache();
  const createSet = useCreateNamedSet(collection);
  const renameSet = useRenameNamedSet(collection);
  const deleteSet = useDeleteNamedSet(collection);
  const members = useSetNamedSetMembers(collection);

  const create = createSet.mutateAsync;
  const rename = renameSet.mutateAsync;
  const remove = deleteSet.mutateAsync;
  const setMembers = members.mutateAsync;
  const pending =
    createSet.isPending || renameSet.isPending || deleteSet.isPending || members.isPending;

  // Each mutate call resets that mutation's own error and stamps a fresh
  // `submittedAt`, so whichever of the four last started is also whichever
  // last settled; its error (or lack of one) is the collection's error. A
  // fixed create-then-rename-then-remove-then-setMembers order would instead
  // let an old create failure outlive every write that came after it.
  const latest = [createSet, renameSet, deleteSet, members].reduce((newest, next) =>
    next.submittedAt > newest.submittedAt ? next : newest,
  );
  const error = latest.error;

  // Memoised on the mutation objects' own stable `mutateAsync` identities
  // only: `pending` and `error` change on every keystroke of a write, and a
  // caller that lists this object's methods in a `useEffect` dependency
  // array (as `ContactList.tsx` does) must not see a new function each time.
  const callbacks = useMemo(
    () => ({
      create: async (name: string) => (await create(name)).name,
      rename: async (from: string, to: string) => (await rename({ from, to })).name,
      remove: (name: string) => remove(name),
      setMembers: (name: string, patch: MembersPatch) => setMembers({ name, patch }),
      invalidate: () => cache.invalidate(collection.key),
    }),
    [create, rename, remove, setMembers, cache, collection.key],
  );

  return { ...callbacks, pending, error };
}
