import { messageTags, tagSlug } from "../lib/messageTags";
import { TagIcon } from "./icons";
import NavEntityList, { type NavEntityCopy } from "./NavEntityList";

const COPY: NavEntityCopy = {
  id: "message-tags",
  title: "Message Tags",
  routeBase: "/tag",
  emptyRoute: "/no-tag",
  emptyLabel: "No Message Tag",
  fallbackRoute: "/",
  addLabel: "Create Message Tag",
  createTitle: "Create Message Tag",
  renameTitle: "Rename Message Tag",
  namePlaceholder: "Message Tag name",
  optionsLabel: (name) => `Message Tag options for ${name}`,
  deleteBody: (name) =>
    `Removes the Message Tag ${name} and takes it off every conversation that carries it. The conversations themselves stay in your Message Crate.`,
  createError: "Could not create Message Tag",
  renameError: "Could not rename Message Tag",
  deleteError: "Could not delete Message Tag",
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
