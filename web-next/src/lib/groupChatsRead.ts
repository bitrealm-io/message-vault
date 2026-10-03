import { currentAccountId } from "./accountScope";
import { loadAccountProfile } from "./accountProfile";
import {
  combinedDedupeSql,
  getDb,
  handleIdsForRaws,
  hasDuplicateOfColumn,
  hasTrashedConversationsTable,
  looksLikePhone,
  resetDb,
  usefulNameAlias,
} from "./dbCore";
import type { HandleType } from "./handleKind";
import { formatPhoneDisplay } from "./phoneE164";
import type { GroupChatThread, GroupParticipant, GroupYearRow } from "./types";

const MAX_VISIBLE_NAMES = 8;

function isGenericGroupTitle(title: string | null | undefined): boolean {
  if (!title) return true;
  const t = title.trim();
  if (!t) return true;
  // iMessage chat identifiers look like chat31771234567890...
  if (/^chat\d+/i.test(t)) return true;

  // SMS Backup-style titles that are only phone numbers, e.g.
  // "Group: +14075550102, +14075550103, and 6 others"
  let rest = t.replace(/^group:\s*/i, "").trim();
  rest = rest.replace(/,?\s*and\s+\d+\s+others?\.?$/i, "").trim();
  if (!rest) return true;

  const parts = rest
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length > 0 && parts.every(looksLikePhone)) return true;

  return false;
}

function participantLabel(row: {
  preferred_name?: string | null;
  name_alias: string | null;
  handle: string;
}): { name: string; unknown: boolean } {
  const preferred = row.preferred_name?.trim() ?? "";
  if (preferred) return { name: preferred, unknown: false };
  const hint = usefulNameAlias(row.name_alias, row.handle);
  if (hint) return { name: hint, unknown: false };
  return { name: formatPhoneDisplay(row.handle), unknown: true };
}

function formatPeopleTitle(
  labels: Array<{ name: string; unknown: boolean }>,
): {
  short: string;
  full: string;
  count: number;
} {
  const seen = new Set<string>();
  const unique: Array<{ name: string; unknown: boolean }> = [];
  for (const label of labels) {
    if (!label.name || seen.has(label.name)) continue;
    seen.add(label.name);
    unique.push(label);
  }

  unique.sort((a, b) => {
    if (a.unknown !== b.unknown) return a.unknown ? 1 : -1;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });

  const names = unique.map((l) => l.name.replace(/ /g, "\u00a0"));
  const sep = "\u00a0\u00a0·\u00a0\u00a0";
  const full = names.join(sep);
  if (names.length === 0) {
    return { short: "Group message", full: "Group message", count: 0 };
  }
  if (names.length <= MAX_VISIBLE_NAMES) {
    return { short: full, full, count: names.length };
  }
  const shown = names.slice(0, MAX_VISIBLE_NAMES).join(sep);
  return {
    short: `${shown}\u00a0\u00a0+${names.length - MAX_VISIBLE_NAMES}`,
    full,
    count: names.length,
  };
}

type GroupPeopleTitle = {
  title: string;
  titleFull: string;
  namedTitle: string | null;
  participantCount: number;
  participantNames: string[];
  participantHandles: string[];
  participants: GroupParticipant[];
};

/** Resolve people labels for group conversations, excluding owner (+ optional focus contact). */
function groupPeopleTitles(
  conversationIds: number[],
  excludePhones: string[] = [],
): Map<number, GroupPeopleTitle> {
  const out = new Map<number, GroupPeopleTitle>();
  if (!conversationIds.length) return out;

  const accountId = currentAccountId();
  const db = getDb();
  const owner = loadAccountProfile(accountId);
  const exclude = new Set(
    [...owner.phones, ...excludePhones].filter(Boolean).map((p) => p.trim()),
  );

  const placeholders = conversationIds.map(() => "?").join(",");
  const meta = db
    .prepare(
      `SELECT id, group_title FROM conversations
       WHERE account_id = ? AND id IN (${placeholders})`,
    )
    .all(accountId, ...conversationIds) as Array<{ id: number; group_title: string | null }>;
  const namedById = new Map(
    meta.map((r) => [
      r.id,
      isGenericGroupTitle(r.group_title) ? null : (r.group_title?.trim() ?? null),
    ]),
  );

  const rows = db
    .prepare(
      `SELECT p.conversation_id,
              ph.raw AS handle,
              ph.handle_type AS handle_type,
              p.name_alias,
              c.id AS contact_id, c.preferred_name
       FROM participants p
       JOIN conversations conv ON conv.id = p.conversation_id
       JOIN handles ph ON ph.id = p.handle_id
       LEFT JOIN contact_handles cp ON cp.handle_id = p.handle_id AND cp.account_id = conv.account_id
       LEFT JOIN contacts c ON c.id = cp.contact_id AND c.account_id = cp.account_id
       WHERE conv.account_id = ? AND p.conversation_id IN (${placeholders})`,
    )
    .all(accountId, ...conversationIds) as Array<{
    conversation_id: number;
    handle: string;
    handle_type: string | null;
    name_alias: string | null;
    contact_id: number | null;
    preferred_name: string | null;
  }>;

  const byConv = new Map<
    number,
    Array<{
      name: string;
      unknown: boolean;
      handle: string;
      handleType: HandleType | null;
      contactId: number | null;
    }>
  >();
  for (const r of rows) {
    const handle = r.handle.trim();
    if (exclude.has(handle)) continue;
    const list = byConv.get(r.conversation_id) ?? [];
    list.push({
      ...participantLabel(r),
      handle,
      handleType: (r.handle_type as HandleType | null) ?? null,
      contactId: r.contact_id ?? null,
    });
    byConv.set(r.conversation_id, list);
  }

  for (const id of conversationIds) {
    const entries = byConv.get(id) ?? [];
    const people = formatPeopleTitle(entries);
    const namedTitle = namedById.get(id) ?? null;

    const handleSeen = new Set<string>();
    const participants: GroupParticipant[] = [];
    for (const e of entries) {
      if (!e.handle || handleSeen.has(e.handle)) continue;
      handleSeen.add(e.handle);
      participants.push({
        name: e.name,
        handle: e.handle,
        handleType: e.handleType,
        contactId: e.contactId,
      });
    }
    participants.sort((a, b) => {
      const aUnknown = a.contactId == null && a.name === a.handle;
      const bUnknown = b.contactId == null && b.name === b.handle;
      if (aUnknown !== bUnknown) return aUnknown ? 1 : -1;
      return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    });

    out.set(id, {
      title: people.short,
      titleFull: namedTitle ? `${namedTitle}\n${people.full}` : people.full,
      namedTitle,
      participantCount: people.count,
      participantNames: participants.map((p) => p.name),
      participantHandles: participants.map((p) => p.handle),
      participants,
    });
  }
  return out;
}

export function contactGroupChatThreadsForPhones(
  phones: string[],
  source?: string | null,
): GroupChatThread[] {
  return contactGroupChatThreadsForPhoneSets([phones], source);
}

/**
 * Group chats where each phone set appears as a participant (AND).
 * Extra participants outside these sets are allowed.
 */
export function contactGroupChatThreadsForPhoneSets(
  phoneSets: string[][],
  source?: string | null,
): GroupChatThread[] {
  const sets = phoneSets
    .map((phones) => [
      ...new Set(phones.map((p) => p.trim()).filter(Boolean)),
    ])
    .filter((phones) => phones.length > 0);
  if (!sets.length) return [];

  const accountId = currentAccountId();
  const db = getDb();
  const sourceSql = source ? " AND m.source = ?" : "";
  const params: Array<string | number> = [accountId];
  const existsSql = sets
    .map((phones) => {
      const ids = handleIdsForRaws(db, accountId, phones);
      if (ids.length === 0) {
        return `AND 1 = 0`;
      }
      const placeholders = ids.map(() => "?").join(",");
      params.push(...ids);
      return `AND EXISTS (
           SELECT 1 FROM participants p
           WHERE p.conversation_id = c.id AND p.handle_id IN (${placeholders})
         )`;
    })
    .join("\n         ");
  if (source) params.push(source);
  const hasTrash = hasTrashedConversationsTable(db);
  const trashFilter = hasTrash
    ? `AND NOT EXISTS (
         SELECT 1 FROM trashed_conversations tc
         WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
       )`
    : "";
  const excludePhones = sets.flat();
  const rows = db
    .prepare(
      `SELECT c.id AS conversation_id,
              CAST(substr(m.timestamp, 1, 4) AS INTEGER) AS year,
              COUNT(*) AS message_count,
              MIN(substr(m.timestamp, 1, 10)) AS date_start,
              MAX(substr(m.timestamp, 1, 10)) AS date_end
       FROM conversations c
       JOIN messages m ON m.conversation_id = c.id
       WHERE c.account_id = ?
         AND c.conversation_type = 'group'
         ${existsSql}${trashFilter}${sourceSql}${combinedDedupeSql(source, "m")}
       GROUP BY c.id, year
       HAVING message_count > 0
       ORDER BY year DESC, c.id`,
    )
    .all(...params) as Array<{
    conversation_id: number;
    year: number;
    message_count: number;
    date_start: string;
    date_end: string;
  }>;

  const conversationIds = [...new Set(rows.map((r) => r.conversation_id))];
  const titles = groupPeopleTitles(conversationIds, excludePhones);
  const fingerprints = groupParticipantFingerprints(conversationIds);

  const mapped = rows.map((r) => {
    const t = titles.get(r.conversation_id) ?? {
      title: "Group message",
      titleFull: "Group message",
      namedTitle: null,
      participantCount: 0,
      participantNames: [] as string[],
      participantHandles: [] as string[],
      participants: [] as GroupParticipant[],
    };
    return {
      conversationId: r.conversation_id,
      conversationIds: [r.conversation_id],
      title: t.title,
      titleFull: t.titleFull,
      namedTitle: t.namedTitle,
      participantCount: t.participantCount,
      participantNames: t.participantNames,
      participantHandles: t.participantHandles,
      participants: t.participants,
      year: r.year,
      messageCount: r.message_count,
      dateStart: r.date_start,
      dateEnd: r.date_end,
      fingerprint: fingerprints.get(r.conversation_id) ?? `id:${r.conversation_id}`,
    };
  });

  // Combined view: collapse the same people across exporters into one row.
  if (source) {
    return mapped.map(({ fingerprint, ...rest }) => {
      void fingerprint;
      return rest;
    });
  }

  const byKey = new Map<string, (typeof mapped)[number]>();
  for (const row of mapped) {
    const key = `${row.year}|${row.fingerprint}`;
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, { ...row, conversationIds: [...row.conversationIds] });
      continue;
    }
    for (const id of row.conversationIds) {
      if (!existing.conversationIds.includes(id)) {
        existing.conversationIds.push(id);
      }
    }
    existing.messageCount += row.messageCount;
    if (row.dateStart < existing.dateStart) existing.dateStart = row.dateStart;
    if (row.dateEnd > existing.dateEnd) existing.dateEnd = row.dateEnd;
    if (row.participantCount > existing.participantCount) {
      existing.participantCount = row.participantCount;
      existing.title = row.title;
      existing.titleFull = row.titleFull;
      existing.namedTitle = row.namedTitle;
      existing.participantNames = row.participantNames;
      existing.participantHandles = row.participantHandles;
      existing.participants = row.participants;
    }
  }

  // Recount soft-deduped messages across merged conversation ids so we don't
  // sum pre-dedupe copies when duplicate_of is not yet set / partially set.
  const merged = [...byKey.values()].map(({ fingerprint, ...rest }) => {
    void fingerprint;
    if (rest.conversationIds.length === 1) return rest;
    const stats = groupYearStatsForConversations(
      rest.conversationIds,
      rest.year,
      source,
    );
    return {
      ...rest,
      conversationId: rest.conversationIds[0]!,
      messageCount: stats.messageCount,
      dateStart: stats.dateStart || rest.dateStart,
      dateEnd: stats.dateEnd || rest.dateEnd,
    };
  });

  merged.sort((a, b) => b.year - a.year || a.conversationId - b.conversationId);
  return merged;
}

function groupParticipantFingerprints(
  conversationIds: number[],
): Map<number, string> {
  const out = new Map<number, string>();
  if (!conversationIds.length) return out;
  const accountId = currentAccountId();
  const db = getDb();
  const placeholders = conversationIds.map(() => "?").join(",");
  const rows = db
    .prepare(
      `SELECT p.conversation_id, ph.raw AS handle
       FROM participants p
       JOIN conversations c ON c.id = p.conversation_id
       JOIN handles ph ON ph.id = p.handle_id
       WHERE c.account_id = ? AND p.conversation_id IN (${placeholders})
         AND ph.raw IS NOT NULL AND ph.raw != ''
       ORDER BY p.conversation_id, ph.raw`,
    )
    .all(accountId, ...conversationIds) as Array<{ conversation_id: number; handle: string }>;
  const byConv = new Map<number, string[]>();
  for (const r of rows) {
    const list = byConv.get(r.conversation_id) ?? [];
    list.push(r.handle.trim());
    byConv.set(r.conversation_id, list);
  }
  for (const id of conversationIds) {
    const handles = [...new Set(byConv.get(id) ?? [])].sort();
    out.set(id, handles.length ? `group:${handles.join("|")}` : `id:${id}`);
  }
  return out;
}

function groupYearStatsForConversations(
  conversationIds: number[],
  year: number,
  source?: string | null,
): { messageCount: number; dateStart: string; dateEnd: string } {
  const accountId = currentAccountId();
  const db = getDb();
  const placeholders = conversationIds.map(() => "?").join(",");
  const sourceSql = source ? " AND m.source = ?" : "";
  const params: Array<string | number> = [
    accountId,
    ...conversationIds,
    `${year}-`,
    `${year + 1}-`,
  ];
  if (source) params.push(source);
  const row = db
    .prepare(
      `SELECT COUNT(*) AS message_count,
              MIN(substr(m.timestamp, 1, 10)) AS date_start,
              MAX(substr(m.timestamp, 1, 10)) AS date_end
       FROM messages m
       JOIN conversations c ON c.id = m.conversation_id
       WHERE c.account_id = ? AND m.conversation_id IN (${placeholders})
         AND m.timestamp >= ? AND m.timestamp < ?${sourceSql}${combinedDedupeSql(source, "m")}`,
    )
    .get(...params) as {
    message_count: number;
    date_start: string | null;
    date_end: string | null;
  };
  return {
    messageCount: row.message_count,
    dateStart: row.date_start ?? "",
    dateEnd: row.date_end ?? "",
  };
}

/** Group chats split by calendar year for the Groups page list. */
export function listGroupYearRows(): GroupYearRow[] {
  // Re-open after API writes (trash/restore) so RSC sees committed rows.
  resetDb();
  return listGroupYearRowsSection("active");
}

/** Trashed group chats split by calendar year. */
export function listTrashedGroupYearRows(): GroupYearRow[] {
  resetDb();
  return listGroupYearRowsSection("trash");
}



function listGroupYearRowsSection(
  section: "active" | "trash",
): GroupYearRow[] {
  const accountId = currentAccountId();
  const db = getDb();
  const joinDupes = hasDuplicateOfColumn()
    ? " AND m.duplicate_of IS NULL"
    : "";
  const hasTrash = hasTrashedConversationsTable(db);
  if (section === "trash" && !hasTrash) return [];

  const trashFilter = !hasTrash
    ? ""
    : section === "trash"
      ? `AND EXISTS (
           SELECT 1 FROM trashed_conversations tc
           WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
         )`
      : `AND NOT EXISTS (
           SELECT 1 FROM trashed_conversations tc
           WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
         )`;

  const trashedAtSelect =
    section === "trash" && hasTrash
      ? `, (
           SELECT tc.trashed_at FROM trashed_conversations tc
           WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
           LIMIT 1
         ) AS trashed_at`
      : `, NULL AS trashed_at`;

  const rows = db
    .prepare(
      `SELECT c.id,
              CAST(substr(m.timestamp, 1, 4) AS INTEGER) AS year,
              COUNT(m.id) AS message_count,
              MIN(substr(m.timestamp, 1, 10)) AS date_start,
              MAX(substr(m.timestamp, 1, 10)) AS date_end
              ${trashedAtSelect}
       FROM conversations c
       JOIN messages m ON m.conversation_id = c.id${joinDupes}
       WHERE c.account_id = ?
         AND c.conversation_type = 'group'
         ${trashFilter}
       GROUP BY c.id, year
       HAVING message_count > 0`,
    )
    .all(accountId) as Array<{
    id: number;
    year: number;
    message_count: number;
    date_start: string;
    date_end: string;
    trashed_at: string | null;
  }>;

  if (!rows.length) return [];

  const conversationIds = [...new Set(rows.map((r) => r.id))];
  const titles = groupPeopleTitles(conversationIds);

  const yearsByConv = new Map<number, Set<number>>();
  const rangeByConv = new Map<
    number,
    { dateStart: string; dateEnd: string }
  >();
  for (const r of rows) {
    const years = yearsByConv.get(r.id) ?? new Set<number>();
    years.add(r.year);
    yearsByConv.set(r.id, years);

    const range = rangeByConv.get(r.id);
    if (!range) {
      rangeByConv.set(r.id, {
        dateStart: r.date_start,
        dateEnd: r.date_end,
      });
    } else {
      if (r.date_start < range.dateStart) range.dateStart = r.date_start;
      if (r.date_end > range.dateEnd) range.dateEnd = r.date_end;
    }
  }

  const emptyTitle = {
    title: "Group message",
    titleFull: "Group message",
    namedTitle: null as string | null,
    participantCount: 0,
    participantNames: [] as string[],
    participantHandles: [] as string[],
    participants: [] as GroupParticipant[],
  };

  const items: GroupYearRow[] = rows.map((r) => {
    const t = titles.get(r.id) ?? emptyTitle;
    const range = rangeByConv.get(r.id)!;
    const yearCount = yearsByConv.get(r.id)?.size ?? 1;
    return {
      id: r.id,
      year: r.year,
      title: t.title,
      titleFull: t.titleFull,
      namedTitle: t.namedTitle,
      participantCount: t.participantCount,
      participantNames: t.participantNames,
      participantHandles: t.participantHandles,
      participants: t.participants,
      messageCount: r.message_count,
      dateStart: r.date_start,
      dateEnd: r.date_end,
      conversationDateStart: range.dateStart,
      conversationDateEnd: range.dateEnd,
      spansMultipleYears: yearCount > 1,
      ...(r.trashed_at ? { trashedAt: r.trashed_at } : {}),
    };
  });

  items.sort((a, b) => {
    if (a.year !== b.year) return b.year - a.year;
    const byEnd = b.dateEnd.localeCompare(a.dateEnd);
    if (byEnd !== 0) return byEnd;
    const byStart = b.dateStart.localeCompare(a.dateStart);
    if (byStart !== 0) return byStart;
    return a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
  });
  return items;
}


export function countGroupChats(): number {
  const accountId = currentAccountId();
  const db = getDb();
  const hasTrash = hasTrashedConversationsTable(db);
  const trashFilter = hasTrash
    ? `AND NOT EXISTS (
         SELECT 1 FROM trashed_conversations tc
         WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
       )`
    : "";
  const joinDupes = hasDuplicateOfColumn()
    ? " AND m.duplicate_of IS NULL"
    : "";
  const groupsRow = db
    .prepare(
      `SELECT COUNT(*) AS n FROM (
         SELECT c.id
         FROM conversations c
         LEFT JOIN messages m ON m.conversation_id = c.id${joinDupes}
         WHERE c.account_id = ?
           AND c.conversation_type = 'group'
           ${trashFilter}
         GROUP BY c.id
         HAVING COUNT(m.id) > 0
       )`,
    )
    .get(accountId) as { n: number };
  return groupsRow.n;
}
