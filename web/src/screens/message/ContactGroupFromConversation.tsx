import { useState } from "react";
import GroupNameDialog from "../../components/GroupNameDialog";
import { contactGroups, useContactGroupActions } from "../../lib/contactGroups";
import { useNameCollection } from "../../lib/nameCollection";
import type { Conversation } from "../../lib/types";
import { useContactGroupMembers } from "./contactGroupMembers";

/**
 * "Make a Contact Group from these people": a group chat is a grouping the
 * person already made in real life, so it is the one place a Contact Group
 * can be created from something other than a hand-picked selection (#322).
 *
 * The account owner is left out: a group of "the people I text with" does
 * not contain the person doing the texting. Participants the server has no
 * contact for cannot be members and are left out too.
 *
 * The conversation panel's ⋯ menu opens the name dialog; this draws it, and
 * says what was added once it is saved.
 */
export default function ContactGroupFromConversation({
  conversation,
  open,
  onClose,
}: {
  conversation: Conversation;
  open: boolean;
  onClose: () => void;
}) {
  const memberIds = useContactGroupMembers(conversation);
  const { names } = useNameCollection(contactGroups);
  const actions = useContactGroupActions();
  const [done, setDone] = useState<{ name: string; count: number } | null>(null);

  if (memberIds.length === 0) return null;

  const save = async (name: string) => {
    const trimmed = name.trim();
    const existing = names.find((n) => n.toLowerCase() === trimmed.toLowerCase());
    const target = existing ?? (await actions.create(trimmed));
    await actions.setMembers(target, { add: memberIds });
    setDone({ name: target, count: memberIds.length });
    onClose();
  };

  return (
    <>
      {done ? (
        <span role="status" className="text-[0.75rem] text-muted">
          Added {done.count} people to {done.name}.
        </span>
      ) : null}
      {open ? (
        <GroupNameDialog
          title="Make a Contact Group from these people"
          placeholder="Contact Group name"
          confirmLabel="Create"
          initial={conversation.label ?? ""}
          error={actions.error?.message ?? null}
          busy={actions.pending}
          onSave={save}
          onCancel={onClose}
        />
      ) : null}
    </>
  );
}
