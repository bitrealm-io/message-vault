import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ApiError } from "../lib/api";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { type ContactDetail, useContactDetail, useUpdateContact } from "../lib/contactDetail";
import { contactLabelText } from "../lib/contactLabel";
import { useTrashContact } from "../lib/trash";
import { UNKNOWN_GROUP_LABEL } from "../lib/unknownGroup";
import Button from "./Button";
import ContactLabel from "./ContactLabel";
import { ContactDrawerHandles } from "./contactDrawer/ContactDrawerHandles";
import {
  type ContactBrowseKind,
  type ContactPreview,
  previewHandleStubRows,
} from "./contactDrawer/contactDrawerTypes";
import { PencilIcon } from "./icons";

/**
 * Overlay mode only: dock to the right edge of the list column.
 * Skips setState when the measured edge is unchanged to avoid jitter.
 */
function useDrawerLeft(open: boolean): number | null {
  const [left, setLeft] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setLeft(null);
      return;
    }

    let frame = 0;
    let observer: ResizeObserver | null = null;

    const measure = () => {
      const col = document.querySelector<HTMLElement>("[data-list-column]");
      if (!col) {
        setLeft(null);
        return null;
      }
      const next = Math.round(col.getBoundingClientRect().right);
      setLeft((prev) => (prev === next ? prev : next));
      return col;
    };

    const col = measure();
    if (col) {
      observer = new ResizeObserver(() => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(measure);
      });
      observer.observe(col);
    }

    const onResize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    };
    window.addEventListener("resize", onResize);

    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", onResize);
      observer?.disconnect();
    };
  }, [open]);

  return left;
}

type ContactDrawerProps = {
  contactId: string | null;
  preview?: ContactPreview | null;
  onClose: () => void;
  onBrowseConversations?: (args: {
    contactId: string;
    kind: ContactBrowseKind;
    handle?: string;
  }) => void;
  /** `docked` = flex sibling (contacts page). `overlay` = fixed panel (e.g. from messages). */
  variant?: "docked" | "overlay";
};

/**
 * The drawer stays mounted while the person clicks from one contact to the
 * next, so it is keyed by contact id: each contact starts with its own name
 * editor and its own Move to trash. A Move to trash still answering for the
 * last contact then neither shows its error here nor closes the drawer of the
 * contact now open.
 */
export default function ContactDrawer(props: ContactDrawerProps) {
  return <OneContactDrawer key={props.contactId ?? ""} {...props} />;
}

function OneContactDrawer({
  contactId,
  preview = null,
  onClose,
  onBrowseConversations,
  variant = "overlay",
}: ContactDrawerProps) {
  const updateContact = useUpdateContact();
  const trashContact = useTrashContact();
  const {
    detail: matchedDetail,
    error: detailError,
    retry: retryDetail,
  } = useContactDetail(contactId);
  const [editingName, setEditingName] = useState(false);
  const [nameValue, setNameValue] = useState("");
  // Why the last save of the name was refused. The editor stays open with it.
  const [nameError, setNameError] = useState<string | null>(null);
  const nameEditorRef = useRef<HTMLDivElement>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const savingNameRef = useRef(false);
  const drawerLeft = useDrawerLeft(variant === "overlay" && !!contactId);

  const detailMatches = !!matchedDetail;
  // A failed refetch behind a contact already on screen keeps showing it; only
  // a drawer with nothing to show says the load failed.
  const loadError = detailMatches ? null : detailError;
  const previewMatches = !!contactId && !!preview && String(preview.id) === String(contactId);
  const matchedName = matchedDetail?.name;

  const displayName = detailMatches
    ? matchedDetail.name
    : previewMatches
      ? preview?.name
      : loadError
        ? "Contact"
        : "Loading…";
  const loading = !detailMatches && !loadError;

  useEffect(() => {
    setNameValue(displayName === "Loading…" ? "" : displayName);
    setEditingName(false);
  }, [displayName]);

  const cancelEdit = useCallback(() => {
    if (savingNameRef.current) return;
    setEditingName(false);
    if (matchedName != null) {
      setNameValue(matchedName);
    }
  }, [matchedName]);

  useEffect(() => {
    if (!editingName) {
      savingNameRef.current = false;
      setNameError(null);
    }
  }, [editingName]);

  useEffect(() => {
    if (!contactId) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (editingName) {
        cancelEdit();
        return;
      }
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [contactId, editingName, cancelEdit, onClose]);

  useEffect(() => {
    if (!contactId || !editingName) return;
    const onPointerDown = (e: PointerEvent) => {
      if (savingNameRef.current) return;
      const root = nameEditorRef.current;
      if (!root) return;
      if (e.target instanceof Node && root.contains(e.target)) return;
      cancelEdit();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [contactId, editingName, cancelEdit]);

  useEffect(() => {
    if (!editingName) return;
    const frame = requestAnimationFrame(() => {
      const input = nameInputRef.current;
      if (!input) return;
      input.focus();
      input.select();
    });
    return () => cancelAnimationFrame(frame);
  }, [editingName]);

  if (!contactId) return null;

  const handleRows: ContactDetail["identities"] = detailMatches
    ? matchedDetail.identities
    : previewMatches
      ? previewHandleStubRows(preview?.addresses, preview?.handleCount)
      : [];

  // null = membership unknown (loading, no preview groups); [] = known empty.
  const storedGroups: string[] | null = detailMatches
    ? (matchedDetail.groups ?? [])
    : previewMatches && preview?.groups != null
      ? preview.groups
      : loading
        ? null
        : [];
  // Unknown is the one group the server computes, so it arrives as a flag
  // beside the stored group names and leads them here.
  const isUnknown = detailMatches ? matchedDetail.unknown : previewMatches && !!preview?.unknown;
  const displayGroups =
    storedGroups && isUnknown ? [UNKNOWN_GROUP_LABEL, ...storedGroups] : storedGroups;

  const browse = (args: { kind: ContactBrowseKind; handle?: string }) => {
    if (!onBrowseConversations || !contactId) return;
    onBrowseConversations({ contactId, kind: args.kind, handle: args.handle });
  };

  // The contact just left the list this drawer was opened from, so close it
  // rather than leave the person looking at a contact that has quietly gone.
  const handleMoveToTrash = () => {
    trashContact.mutate(contactId, { onSuccess: onClose });
  };

  const saveName = async () => {
    if (savingNameRef.current) return;
    savingNameRef.current = true;
    setNameError(null);
    try {
      if (!detailMatches || nameValue === matchedDetail?.name) {
        setEditingName(false);
        return;
      }
      await updateContact.mutateAsync({ contactId, body: { name: nameValue } });
      setEditingName(false);
    } catch (err) {
      savingNameRef.current = false;
      setNameError(apiErrorMessage(err, "Could not save the name"));
    }
  };

  const panelClass =
    variant === "docked"
      ? "flex h-full min-h-0 min-w-0 flex-col overflow-auto [scrollbar-gutter:stable] bg-panel px-6 pb-6 pt-2 outline-none"
      : "fixed top-0 bottom-0 z-40 w-[min(920px,calc(100vw-14rem))] overflow-auto [scrollbar-gutter:stable] border-l border-border bg-panel p-6 shadow-[2px_0_12px_rgba(0,0,0,0.18)] outline-none";

  const panelStyle =
    variant === "overlay"
      ? {
          left: drawerLeft ?? undefined,
          right: drawerLeft == null ? 0 : undefined,
        }
      : undefined;

  if (loadError) {
    return (
      <aside role="dialog" aria-label={displayName} className={panelClass} style={panelStyle}>
        <ContactLoadFailed
          name={displayName}
          error={loadError}
          onRetry={retryDetail}
          onClose={onClose}
        />
      </aside>
    );
  }

  return (
    <aside
      role="dialog"
      aria-label={contactLabelText(
        displayName ?? "",
        handleRows.map((h) => h.address),
      )}
      aria-busy={loading || undefined}
      className={panelClass}
      style={panelStyle}
    >
      <ContactDrawerHandles
        contactId={contactId}
        handleRows={handleRows}
        conversations={
          detailMatches ? matchedDetail.direct_conversations + matchedDetail.group_conversations : 0
        }
        loading={loading}
        onBrowse={onBrowseConversations ? browse : undefined}
        title={
          editingName && detailMatches ? (
            <div ref={nameEditorRef} className="w-max min-w-[8rem] max-w-[50%]">
              <input
                ref={nameInputRef}
                type="text"
                value={nameValue}
                size={Math.max(nameValue.length + 1, 8)}
                aria-label="Contact name"
                title="Press Enter to save, Escape to cancel"
                onChange={(e) => setNameValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void saveName();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    cancelEdit();
                  }
                }}
                onBlur={() => {
                  cancelEdit();
                }}
                className="box-border h-7 w-full min-w-0 rounded border border-border bg-elevated px-1.5 py-0 text-[1.125rem] font-semibold leading-none text-text"
              />
              {nameError ? (
                <p role="alert" className="m-0 mt-1 text-[0.813rem] font-normal text-danger">
                  {nameError}
                </p>
              ) : null}
            </div>
          ) : (
            <div className="flex min-w-0 items-center gap-2">
              <h2 className="m-0 min-w-0 truncate text-[1.125rem] font-semibold">
                {detailMatches || previewMatches ? (
                  <ContactLabel
                    name={displayName ?? ""}
                    addresses={handleRows.map((h) => h.address)}
                  />
                ) : (
                  displayName
                )}
              </h2>
              <Button
                variant="ghostNeutral"
                size="icon"
                title="Edit name"
                aria-label="Edit name"
                disabled={!detailMatches}
                onClick={() => setEditingName(true)}
              >
                <PencilIcon />
              </Button>
            </div>
          )
        }
        intro={
          <div>
            {trashContact.error && (
              <div className="mb-3 rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.75rem] text-danger">
                {apiErrorMessage(trashContact.error, "Could not move this contact to trash.")}
              </div>
            )}
            <div className="mb-1.5">
              <span className="text-[0.75rem] font-semibold uppercase tracking-[0.04em] text-muted">
                Contact groups
              </span>
            </div>
            <div className="flex min-h-6 flex-wrap items-center gap-1.5">
              {displayGroups == null ? (
                <span className="py-0.5 text-[0.75rem] leading-4 text-muted" aria-hidden>
                  …
                </span>
              ) : displayGroups.length > 0 ? (
                displayGroups.map((name) => (
                  <span
                    key={name}
                    className="rounded-full bg-elevated px-2 py-0.5 text-[0.75rem] leading-4 text-text"
                  >
                    {name}
                  </span>
                ))
              ) : (
                <span className="py-0.5 text-[0.75rem] leading-4 text-muted">No groups</span>
              )}
            </div>
          </div>
        }
        toolbarExtra={
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="ghostNeutral"
              size="sm"
              disabled={trashContact.isPending}
              onClick={handleMoveToTrash}
            >
              {trashContact.isPending ? "Moving to trash…" : "Move to trash"}
            </Button>
            <button
              type="button"
              aria-label="Close"
              onClick={onClose}
              className="cursor-pointer border-none bg-transparent p-0 text-[1.25rem] leading-none text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1"
            >
              ×
            </button>
          </div>
        }
      />
    </aside>
  );
}

/**
 * In place of the drawer's contents while its contact cannot be loaded.
 *
 * `404 Not Found` gets its own sentence: the contact was moved to the Trash or
 * deleted somewhere else, and the server's own message would only say it was
 * not found.
 */
function ContactLoadFailed({
  name,
  error,
  onRetry,
  onClose,
}: {
  name: string;
  error: Error;
  onRetry: () => void;
  onClose: () => void;
}) {
  const gone = error instanceof ApiError && error.status === 404;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="m-0 min-w-0 truncate text-[1.125rem] font-semibold">{name}</h2>
        <button
          type="button"
          aria-label="Close"
          onClick={onClose}
          className="cursor-pointer border-none bg-transparent p-0 text-[1.25rem] leading-none text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1"
        >
          ×
        </button>
      </div>
      <div
        role="alert"
        className="rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.813rem] text-danger"
      >
        {gone ? (
          <p className="m-0">This contact is no longer in your contacts.</p>
        ) : (
          <>
            <p className="m-0 font-semibold">This contact could not be loaded.</p>
            <p className="m-0 mt-1">{apiErrorMessage(error, "The server did not answer.")}</p>
          </>
        )}
      </div>
      <div>
        <Button variant="secondary" size="sm" onClick={onRetry}>
          Try again
        </Button>
      </div>
    </div>
  );
}
