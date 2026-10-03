import { useMemo, useState } from "react";
import GroupNameDialog from "../../components/GroupNameDialog";
import PlainButton from "../../components/PlainButton";
import { contactGroups, useContactGroupActions } from "../../lib/contactGroups";
import { useNameCollection } from "../../lib/nameCollection";
import { phonesMatch } from "../../lib/phoneTokens";
import type { Conversation, Participant } from "../../lib/types";
import { useAccountProfile } from "../../lib/useAccountProfile";

/**
 * "Make a Contact Group from these people": a group chat is a grouping the
 * person already made in real life, so it is the one place a Contact Group
 * can be created from something other than a hand-picked selection (#322).
 *
 * The account owner is left out: a group of "the people I text with" does
 * not contain the person doing the texting. Participants the server has no
 * contact for cannot be members and are left out too.
 */
export default function ContactGroupFromConversation({
  conversation,
}: {
  conversation: Conversation;
}) {
  const { profile } = useAccountProfile();
  const { names } = useNameCollection(contactGroups);
  const actions = useContactGroupActions();
  const [open, setOpen] = useState(false);
  const [done, setDone] = useState<{ name: string; count: number } | null>(null);

  const memberIds = useMemo(
    () => memberContactIds(conversation.participants, profile?.phones ?? [], profile?.emails ?? []),
    [conversation.participants, profile?.phones, profile?.emails],
  );

  if (!conversation.is_group || memberIds.length < 2) return null;

  const save = async (name: string) => {
    const trimmed = name.trim();
    const existing = names.find((n) => n.toLowerCase() === trimmed.toLowerCase());
    const target = existing ?? (await actions.create(trimmed));
    await actions.setMembers(target, { add: memberIds });
    setDone({ name: target, count: memberIds.length });
    setOpen(false);
  };

  return (
    <>
      <PlainButton
        onPress={() => setOpen(true)}
        className="cursor-pointer rounded-full border border-border bg-panel px-2 py-0.5 text-[0.75rem] text-accent"
      >
        Make a Contact Group
      </PlainButton>
      {done ? (
        <span role="status" className="text-[0.75rem] text-muted">
          Added {done.count} people to {done.name}.
        </span>
      ) : null}
      {open ? (
        <GroupNameDialog
          title="Make a Contact Group from these people"
          placeholder="Group name"
          confirmLabel="Create"
          initial={conversation.label ?? ""}
          error={actions.error?.message ?? null}
          busy={actions.pending}
          onSave={save}
          onCancel={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}

/** The distinct contact ids of everyone in the chat except the account owner. */
function memberContactIds(
  participants: readonly Participant[],
  ownerPhones: readonly string[],
  ownerEmails: readonly string[],
): number[] {
  const owner = (handle: string | null | undefined): boolean => {
    if (!handle) return false;
    if (handle.includes("@")) {
      const wanted = handle.trim().toLowerCase();
      return ownerEmails.some((e) => e.trim().toLowerCase() === wanted);
    }
    return ownerPhones.some((p) => phonesMatch(p, handle));
  };
  const ids: number[] = [];
  for (const p of participants) {
    if (p.contact_id == null || owner(p.handle)) continue;
    if (!ids.includes(p.contact_id)) ids.push(p.contact_id);
  }
  return ids;
}
