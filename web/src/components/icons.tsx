import type { ReactNode, SVGProps } from "react";

type IconProps = {
  size?: number;
  className?: string;
} & Omit<SVGProps<SVGSVGElement>, "width" | "height" | "children">;

function IconShell({
  size = 13,
  className,
  children,
  ...rest
}: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className ?? "shrink-0"}
      {...rest}
    >
      {children}
    </svg>
  );
}

/** Edit pencil — diagonal outline with tip and ferrule line. */
export function PencilIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
      <path d="m15 5 4 4" />
    </IconShell>
  );
}

/** Delete / trash — lid with handle, can body, two inner lines. */
export function TrashIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M3 6h18" />
      <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
    </IconShell>
  );
}

/** Plus — horizontal and vertical stroke. */
export function PlusIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </IconShell>
  );
}

/** Chevron pointing right; rotate 90° when a section is open. */
export function ChevronRightIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="m9 6 6 6-6 6" />
    </IconShell>
  );
}

/** Chevron pointing down — a closed menu that opens below. */
export function ChevronDownIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="m6 9 6 6 6-6" />
    </IconShell>
  );
}

/** Three dots for a row menu. */
export function EllipsisIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none" />
    </IconShell>
  );
}

/** Two people — a contact group. */
export function PeopleGroupIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="9" cy="8" r="3" />
      <path d="M3 19c0-2.8 2.7-5 6-5s6 2.2 6 5" />
      <circle cx="17" cy="9" r="2.4" />
      <path d="M16 14.2c2.4.4 4 2.2 4 4.8" />
    </IconShell>
  );
}

/** Price-tag shape — a message tag. */
export function TagIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M20.6 13.4 12 22l-8.6-8.6a2 2 0 0 1 0-2.8L11.2 3H21v9.8a2 2 0 0 1-.4 1.6Z" />
      <circle cx="16.5" cy="7.5" r="1.2" />
    </IconShell>
  );
}

/** Gear — settings. */
export function GearIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
    </IconShell>
  );
}

/** Open door with an arrow out — log out. */
export function SignOutIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <path d="m16 17 5-5-5-5" />
      <path d="M21 12H9" />
    </IconShell>
  );
}

/** Magnifying glass — a saved search. */
export function SearchIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </IconShell>
  );
}

/** One person — contacts with no group. */
export function PersonIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="12" cy="8" r="3" />
      <path d="M5 20c0-3.3 3.1-6 7-6s7 2.7 7 6" />
    </IconShell>
  );
}

/** Check mark — a tool was found. */
export function CheckIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M20 6 9 17l-5-5" />
    </IconShell>
  );
}

/** X mark — a tool was not found. */
export function XIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </IconShell>
  );
}

/** Padlock — body and shackle. Marks a password field. */
export function LockIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <rect x="4" y="10.5" width="16" height="10" rx="2" />
      <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
    </IconShell>
  );
}

/** Handset — marks an account reached by phone number rather than by address. */
export function PhoneIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      {/* The handset is drawn from y=3.6 to y=16, so its own middle is 2.2
          above the middle of the 24-unit box. Centring the box therefore left
          the glyph sitting high in whatever it was placed in; the shift puts
          the glyph itself on the centre line, which is what the eye reads. */}
      <g transform="translate(0 2.2)">
        <path d="M6.6 3.6h2.6l1.3 3.2-1.9 1.1a10.4 10.4 0 0 0 4.6 4.6l1.1-1.9 3.2 1.3v2.6a1.5 1.5 0 0 1-1.6 1.5A13.6 13.6 0 0 1 5.1 5.2a1.5 1.5 0 0 1 1.5-1.6Z" />
      </g>
    </IconShell>
  );
}
