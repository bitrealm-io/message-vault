import type { ReactNode } from "react";
import { Dialog, type DialogProps, Modal, ModalOverlay } from "react-aria-components";
import { Z_DRAWER, Z_DRAWER_SCRIM, Z_MODAL } from "../lib/zLayers";

/**
 * Button row along the bottom of a dialog. Its own component so the spacing is
 * decided once rather than re-typed at each dialog.
 */
export function DialogFooter({ children }: { children: ReactNode }) {
  return <div className="mt-5 flex justify-end gap-2">{children}</div>;
}

const CLOSE_BUTTON =
  "absolute top-3 right-3 cursor-pointer border-none bg-transparent text-[1.25rem] leading-none text-muted disabled:cursor-not-allowed disabled:opacity-50";

/** Error line shown inside a dialog when the action it submits fails. */
export function DialogError({ message }: { message: string }) {
  if (!message) return null;
  return (
    <div
      role="alert"
      className="mt-3 rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.813rem] text-danger"
    >
      {message}
    </div>
  );
}

export default function ModalShell({
  open,
  onOpenChange,
  children,
  maxWidth = "28rem",
  dismissable = true,
  keyboardDismissable = true,
  label,
  variant = "dialog",
  title,
  onClose,
  closeDisabled = false,
  ...dialogProps
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
  maxWidth?: string;
  /** Whether a click outside the dialog closes it. */
  dismissable?: boolean;
  /** Whether Escape closes the dialog. React Aria lets Escape close it even when `dismissable` is false. */
  keyboardDismissable?: boolean;
  label: string;
  /** Centered dialog (default) or right-edge drawer (Sources panel). */
  variant?: "dialog" | "drawer";
  /**
   * Renders the heading and the close “×” for it. Every dialog in the app wants
   * both, so they live here instead of being re-typed — one of the copies had
   * lost its `aria-label` along the way.
   */
  title?: string;
  onClose?: () => void;
  closeDisabled?: boolean;
} & Omit<DialogProps, "children" | "className">) {
  const closeButton = (className: string) =>
    onClose != null ? (
      <button
        type="button"
        aria-label="Close"
        disabled={closeDisabled}
        onClick={onClose}
        className={className}
      >
        ×
      </button>
    ) : null;

  if (variant === "drawer") {
    return (
      <ModalOverlay
        isOpen={open}
        isDismissable={dismissable}
        isKeyboardDismissDisabled={!keyboardDismissable}
        onOpenChange={onOpenChange}
        className={`fixed inset-0 bg-scrim ${Z_DRAWER_SCRIM}`}
      >
        <Modal
          className={`fixed top-0 right-0 bottom-0 w-[320px] overflow-auto bg-panel p-6 shadow-drawer outline-none ${Z_DRAWER}`}
        >
          <Dialog aria-label={label} className="relative outline-none" {...dialogProps}>
            {title != null ? (
              <div className="mb-4 flex justify-between">
                <h2 className="m-0 text-[1.125rem]">{title}</h2>
                {closeButton("cursor-pointer border-none bg-none text-[1.25rem] text-muted")}
              </div>
            ) : null}
            {children}
          </Dialog>
        </Modal>
      </ModalOverlay>
    );
  }

  return (
    <ModalOverlay
      isOpen={open}
      isDismissable={dismissable}
      isKeyboardDismissDisabled={!keyboardDismissable}
      onOpenChange={onOpenChange}
      className={`fixed inset-0 flex items-center justify-center bg-scrim p-4 ${Z_MODAL}`}
    >
      <Modal
        className="relative w-full rounded-lg border border-border bg-panel p-5 shadow-modal outline-none"
        style={{ maxWidth }}
      >
        <Dialog aria-label={label} className="outline-none" {...dialogProps}>
          {closeButton(CLOSE_BUTTON)}
          {title != null ? (
            <h2 className="mb-2 pr-6 text-[1.125rem] font-semibold text-text">{title}</h2>
          ) : null}
          {children}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
