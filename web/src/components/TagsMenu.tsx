import type { MembershipCheckState } from "../lib/membershipChecks";
import { isReservedTagName, reservedTagError } from "../lib/messageTags";
import GroupsMenu from "./GroupsMenu";
import { TagIcon } from "./icons";

/** Assign or remove message tags on the selected conversations. */
export default function TagsMenu({
  allTags,
  checks,
  onToggle,
  onCreate,
  onClearAll,
  disabled = false,
}: {
  allTags: string[];
  checks: Record<string, MembershipCheckState>;
  onToggle?: (name: string) => void;
  onCreate?: (name: string) => void;
  onClearAll?: () => void;
  disabled?: boolean;
}) {
  return (
    <GroupsMenu
      allGroups={allTags}
      checks={checks}
      onToggle={onToggle}
      onCreate={onCreate}
      onClearAll={onClearAll}
      disabled={disabled}
      ariaLabel="Message Tags"
      title="Message Tags"
      searchPlaceholder="Search Message Tags…"
      emptyText="No Message Tags"
      noMatchText="No matching Message Tags"
      createButtonLabel="Create Message Tag"
      createTitle="Create Message Tag"
      createPlaceholder="Message Tag name"
      isReserved={isReservedTagName}
      reservedError={reservedTagError}
      icon={<TagIcon size={16} />}
      labeled={false}
    />
  );
}
