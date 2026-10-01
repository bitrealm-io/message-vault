import { messageTags, tagSlug } from "../lib/messageTags";
import { TagIcon } from "./icons";
import NavEntityList, { type NavEntityCopy } from "./NavEntityList";

const COPY: NavEntityCopy = {
  id: "message-tags",
  title: "Message Tags",
  routeBase: "/tag",
  emptyRoute: "/no-tag",
  emptyLabel: "No tag",
  fallbackRoute: "/",
  addLabel: "Create message tag",
  createTitle: "Create message tag",
  renameTitle: "Rename tag",
  namePlaceholder: "Tag name",
  optionsLabel: (name) => `Tag options for ${name}`,
  deleteBody: (name) =>
    `Removes the Message Tag ${name} and takes it off every conversation that carries it. The conversations themselves stay in your Message Crate.`,
  createError: "Could not create tag",
  renameError: "Could not rename tag",
  deleteError: "Could not delete tag",
};

export default function MessageTagsNav({ tags }: { tags: string[] }) {
  return (
    <NavEntityList
      names={tags}
      collection={messageTags}
      slug={tagSlug}
      icon={<TagIcon size={15} />}
      emptyIcon={<TagIcon size={15} />}
      copy={COPY}
    />
  );
}
