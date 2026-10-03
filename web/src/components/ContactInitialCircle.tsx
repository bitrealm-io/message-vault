import { contactAvatarClass, contactInitials } from "../lib/contactInitials";

/** Colored circle with contact initials (matches web-next BrowseContactRow). */
export default function ContactInitialCircle({
  displayName,
  preferredHandle,
  size = 28,
}: {
  displayName: string;
  preferredHandle?: string | null;
  size?: number;
}) {
  const avatarInput = {
    displayName,
    preferredName: displayName,
    preferredHandle: preferredHandle ?? null,
  };

  return (
    <span
      aria-hidden
      className={`inline-flex shrink-0 select-none items-center justify-center rounded-full font-semibold leading-none text-avatar-text ${contactAvatarClass(avatarInput)}`}
      style={{
        width: size,
        height: size,
        fontSize: size <= 28 ? "11px" : "13px",
      }}
    >
      {contactInitials(avatarInput)}
    </span>
  );
}
