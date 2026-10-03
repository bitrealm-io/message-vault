import { contactGroups, groupSlug } from "../lib/contactGroups";
import { UNKNOWN_GROUP_LABEL } from "../lib/unknownGroup";
import { PeopleGroupIcon, PersonIcon } from "./icons";
import NavEntityList, { type NavEntityCopy } from "./NavEntityList";

const COPY: NavEntityCopy = {
  id: "contact-groups",
  title: "Contact Groups",
  routeBase: "/group",
  emptyRoute: "/no-group",
  emptyLabel: "No Contact Group",
  // Unknown is a permanent group the server computes from contact state: a
  // contact with no identity, or with identities and no preferred name. It
  // empties as the person names or links what is in it.
  permanentRoute: "/unknown",
  permanentLabel: UNKNOWN_GROUP_LABEL,
  fallbackRoute: "/contacts",
  addLabel: "Create Contact Group",
  createTitle: "Create Contact Group",
  renameTitle: "Rename Contact Group",
  namePlaceholder: "Contact Group name",
  optionsLabel: (name) => `Contact Group options for ${name}`,
  deleteBody: (name) =>
    `Removes the Contact Group ${name} and takes every contact out of it. The contacts themselves stay in your Message Crate.`,
  createError: "Could not create Contact Group",
  renameError: "Could not rename Contact Group",
  deleteError: "Could not delete Contact Group",
};

export default function GroupsNav({ groups }: { groups: string[] }) {
  return (
    <NavEntityList
      names={groups}
      collection={contactGroups}
      slug={groupSlug}
      icon={<PeopleGroupIcon size={15} />}
      emptyIcon={<PersonIcon size={15} />}
      copy={COPY}
    />
  );
}
