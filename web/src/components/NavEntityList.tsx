import { type ReactNode, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { slugFromPath, slugPath } from "../lib/contactGroups";
import { type NameCollection, useNameCollectionActions } from "../lib/nameCollection";
import ConfirmDialog from "./ConfirmDialog";
import GroupNameDialog from "./GroupNameDialog";
import { EllipsisIcon } from "./icons";
import NavCollapsibleSection from "./NavCollapsibleSection";
import NavGlyphButton from "./NavGlyphButton";
import {
  NAV_LEADING_GLYPH_CLASS,
  NAV_NESTED_ROW_CLASS,
  navGlyphRowClass,
} from "./navSectionLayout";
import PlainButton from "./PlainButton";
import PopupMenu from "./PopupMenu";

/**
 * The sidebar section listing one named collection: contact groups or message
 * tags. Both were 248-line components differing only in vocabulary, down to a
 * byte-identical copy of `apiErrorMessage`.
 */
export type NavEntityCopy = {
  /** Section id used for the collapse state. */
  id: string;
  /** Section heading, e.g. "Contact Groups". */
  title: string;
  /** Route prefix for one entity, e.g. "/group". */
  routeBase: string;
  /** Route for the "none of these" page, e.g. "/no-group". */
  emptyRoute: string;
  /**
   * A permanent row rendered above the list, for a collection the server
   * computes rather than the person curating. It cannot be renamed or
   * deleted, so it carries no options menu.
   */
  permanentRoute?: string;
  /** Label of the permanent row, e.g. "Unknown". */
  permanentLabel?: string;
  /** Label of the "none of these" row, e.g. "No group". */
  emptyLabel: string;
  /** Where a delete sends the user when they were on the deleted page. */
  fallbackRoute: string;
  addLabel: string;
  createTitle: string;
  renameTitle: string;
  namePlaceholder: string;
  /** Menu button label, completed with the entity name. */
  optionsLabel: (name: string) => string;
  /** What the delete confirmation says is removed and what is kept. */
  deleteBody: (name: string) => string;
  createError: string;
  renameError: string;
  deleteError: string;
};

export default function NavEntityList({
  names,
  collection,
  slug,
  icon,
  emptyIcon,
  copy,
}: {
  names: string[];
  collection: NameCollection;
  slug: (name: string) => string;
  icon: ReactNode;
  emptyIcon: ReactNode;
  copy: NavEntityCopy;
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const actions = useNameCollectionActions(collection);
  const busy = actions.pending;
  const [error, setError] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [renameFor, setRenameFor] = useState<string | null>(null);
  const [deleteFor, setDeleteFor] = useState<string | null>(null);
  // The slug of the set whose page is open, decoded, so a name with spaces
  // or other escaped characters compares as itself.
  const openSlug = slugFromPath(location.pathname, copy.routeBase);

  const create = async (name: string) => {
    if (collection.isReserved(name)) {
      setError(collection.reservedError(name));
      return;
    }
    setError(null);
    try {
      const created = await actions.create(name);
      setCreateOpen(false);
      navigate(slugPath(copy.routeBase, slug(created)));
    } catch (err) {
      setError(apiErrorMessage(err, copy.createError));
    }
  };

  const rename = async (from: string, to: string) => {
    if (collection.isReserved(to)) {
      setError(collection.reservedError(to));
      return;
    }
    setError(null);
    try {
      const next = await actions.rename(from, to);
      setRenameFor(null);
      if (openSlug === slug(from)) {
        navigate(slugPath(copy.routeBase, slug(next)));
      }
    } catch (err) {
      setError(apiErrorMessage(err, copy.renameError));
    }
  };

  const remove = async (name: string) => {
    setError(null);
    try {
      await actions.remove(name);
      setDeleteFor(null);
      if (openSlug === slug(name)) {
        navigate(copy.fallbackRoute);
      }
    } catch (err) {
      setError(apiErrorMessage(err, copy.deleteError));
    }
  };

  return (
    <>
      <NavCollapsibleSection
        id={copy.id}
        title={copy.title}
        addLabel={copy.addLabel}
        addDisabled={busy}
        onAdd={() => {
          setError(null);
          setCreateOpen(true);
        }}
      >
        {copy.permanentRoute && copy.permanentLabel ? (
          <PlainButton
            onPress={() => navigate(copy.permanentRoute as string)}
            className={`${navGlyphRowClass(location.pathname === copy.permanentRoute)} cursor-pointer`}
          >
            <span className={NAV_NESTED_ROW_CLASS}>
              <span className={NAV_LEADING_GLYPH_CLASS}>{emptyIcon}</span>
              <span className="truncate">{copy.permanentLabel}</span>
            </span>
          </PlainButton>
        ) : null}
        {names.map((name) => {
          const href = slugPath(copy.routeBase, slug(name));
          const active = openSlug === slug(name);
          return (
            <div key={name} className="relative w-full">
              <div className={navGlyphRowClass(active)}>
                <PlainButton
                  onPress={() => navigate(href)}
                  className={`${NAV_NESTED_ROW_CLASS} cursor-pointer border-none bg-transparent p-0 text-left text-inherit`}
                >
                  <span className={NAV_LEADING_GLYPH_CLASS}>{icon}</span>
                  <span className="min-w-0 truncate">{name}</span>
                </PlainButton>
                <PopupMenu
                  trigger={
                    <NavGlyphButton
                      aria-label={copy.optionsLabel(name)}
                      disabled={busy}
                      className={
                        active ? "" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                      }
                    >
                      <EllipsisIcon size={15} />
                    </NavGlyphButton>
                  }
                  label={copy.optionsLabel(name)}
                  items={[
                    {
                      label: "Rename…",
                      onSelect: () => {
                        setError(null);
                        setRenameFor(name);
                      },
                    },
                    {
                      label: "Delete",
                      disabled: busy,
                      // Deleting takes the name off everything that carried it,
                      // so it is confirmed first.
                      onSelect: () => {
                        setError(null);
                        setDeleteFor(name);
                      },
                    },
                  ]}
                />
              </div>
            </div>
          );
        })}

        <PlainButton
          onPress={() => navigate(copy.emptyRoute)}
          className={`${navGlyphRowClass(location.pathname === copy.emptyRoute)} cursor-pointer`}
        >
          <span className={NAV_NESTED_ROW_CLASS}>
            <span className={NAV_LEADING_GLYPH_CLASS}>{emptyIcon}</span>
            <span className="truncate">{copy.emptyLabel}</span>
          </span>
        </PlainButton>
      </NavCollapsibleSection>

      {createOpen ? (
        <GroupNameDialog
          title={copy.createTitle}
          placeholder={copy.namePlaceholder}
          confirmLabel="Create"
          error={error}
          busy={busy}
          onSave={create}
          onCancel={() => {
            setCreateOpen(false);
            setError(null);
          }}
        />
      ) : null}
      {renameFor ? (
        <GroupNameDialog
          title={copy.renameTitle}
          placeholder={copy.namePlaceholder}
          initial={renameFor}
          error={error}
          busy={busy}
          onSave={(to) => rename(renameFor, to)}
          onCancel={() => {
            setRenameFor(null);
            setError(null);
          }}
        />
      ) : null}
      <ConfirmDialog
        open={deleteFor !== null}
        title={deleteFor !== null ? `Delete ${deleteFor}?` : ""}
        body={deleteFor !== null ? copy.deleteBody(deleteFor) : ""}
        confirmLabel="Delete"
        danger
        busy={busy}
        error={error ?? ""}
        onClose={() => {
          setDeleteFor(null);
          setError(null);
        }}
        onConfirm={() => {
          if (deleteFor !== null) void remove(deleteFor);
        }}
      />
    </>
  );
}
