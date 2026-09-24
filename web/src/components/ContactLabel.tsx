import type { ReactNode } from "react";
import { contactLabelText } from "../lib/contactLabel";

/**
 * A contact's label. An identity standing in for a missing preferred name is
 * set in italics, so it does not read as a name someone gave the contact.
 * `render` lets a caller mark search matches inside the text.
 */
export default function ContactLabel({
  name,
  addresses,
  render = (text) => text,
}: {
  name: string;
  addresses: readonly string[] | undefined;
  render?: (text: string) => ReactNode;
}) {
  const text = contactLabelText(name, addresses);
  return name.trim() ? render(text) : <em>{render(text)}</em>;
}
