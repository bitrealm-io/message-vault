import { useEffect, useRef, useState } from "react";
import AuthBackButton from "../components/AuthBackButton";
import AuthErrorFooter from "../components/AuthErrorFooter";
import AuthSubmitButton from "../components/AuthSubmitButton";
import Button from "../components/Button";
import { PersonIcon, PhoneIcon } from "../components/icons";
import Select, { ListBoxItem, selectItemClassName } from "../components/Select";
import TextField from "../components/TextField";
import TimeZoneField from "../components/TimeZoneField";
import { useAuth } from "../lib/auth";
import {
  DUPLICATE_HANDLE_MESSAGE,
  HANDLE_SERVICE_OPTIONS,
  HANDLE_SERVICES,
  type HandleService,
  handleDuplicateKey,
  handlePlaceholder,
  handleValidationError,
} from "../lib/handleService";
import { parseSelectKey } from "../lib/selectKey";
import { browserTimeZone } from "../lib/timeZone";
import { authCard, authCardBody, authCardFooter, authTitle, pageCenter } from "../lib/uiStyles";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useAsyncAction } from "../lib/useAsyncAction";
import { updateAccountProfile } from "../lib/vaultApi";

/** The zone an account has until someone chooses one (`accounts.time_zone`'s default). */
const DEFAULT_TIME_ZONE = "UTC";

/**
 * The card never scrolls, and the list of accounts is capped at what fits its
 * floor height so the card does not keep growing under the user as rows are
 * added. Four rows fill that space once the Time Zone picker takes its line;
 * anyone with more finishes the list in Settings → Profile.
 */
const MAX_ACCOUNT_ROWS = 4;

/**
 * How long the error line stays blank before the same message is put back.
 * Long enough that the line is visibly empty for a moment, short enough that
 * the answer still feels like a reply to what was just done.
 *
 * Exported so the test that exercises this blink documents what it is
 * actually waiting on, rather than a bare number.
 */
export const REPEATED_ERROR_BLINK_MS = 250;

/**
 * Checks this close together are one gesture rather than a second look.
 * Releasing the pointer on "+ Add account" both leaves the field and presses
 * the button, so the row is checked twice within a few milliseconds; without
 * this the message would blink on the very first time it appeared. A person
 * clicking a second time is far slower than this.
 *
 * Exported so the test that exercises this gap can advance a fake clock past
 * it deterministically, rather than waiting on the wall clock.
 */
export const SAME_GESTURE_MS = 150;

interface HandleInput {
  id: string;
  handle: string;
  service: HandleService;
}

function newHandleRow(handle = "", service: HandleService = "phone"): HandleInput {
  return { id: crypto.randomUUID(), handle, service };
}

/** Whether `row` still holds `original` exactly, so there is nothing to send for it. */
function sameIdentity(row: HandleInput, original: HandleInput): boolean {
  return row.handle.trim() === original.handle && row.service === original.service;
}

/**
 * Why each row cannot be used, keyed by row id. A row is judged on its own
 * value first; only a value that reads correctly is then compared against the
 * rows above it. The blame for a repeat falls on the later row, since the
 * first one to carry an account is not the mistake.
 */
function rowErrors(rows: HandleInput[]): Map<string, string> {
  const errors = new Map<string, string>();
  const seen = new Set<string>();

  for (const row of rows) {
    const malformed = handleValidationError(row.service, row.handle);
    if (malformed) {
      errors.set(row.id, malformed);
      continue;
    }
    const key = handleDuplicateKey(row.service, row.handle);
    if (!key) continue;
    if (seen.has(key)) errors.set(row.id, DUPLICATE_HANDLE_MESSAGE);
    else seen.add(key);
  }

  return errors;
}

/**
 * A handset for the services reached by phone number, a person for the ones
 * reached by address. The glyph says what kind of thing the field wants before
 * the placeholder does, so it is set slightly larger than the icons that only
 * decorate a label.
 */
function serviceIcon(service: HandleService) {
  return service === "email" ? <PersonIcon size={18} /> : <PhoneIcon size={18} />;
}

export default function OnboardingScreen() {
  const { login, logout, token, serverUrl, accountId } = useAuth();
  const [displayName, setDisplayName] = useState("");
  // Every message time, day and year is shown in this zone, and `date:`
  // searches draw their day boundaries from it. The browser's zone is the
  // right first guess for where this person reads their messages.
  const [timeZone, setTimeZone] = useState(browserTimeZone);
  const [handles, setHandles] = useState<HandleInput[]>(() => [newHandleRow()]);
  // Rows whose value does not read as the kind of account it is set to. Held
  // by id rather than index so removing a row cannot move the mark onto a
  // different one.
  const [invalidIds, setInvalidIds] = useState<string[]>([]);
  const [validationError, setValidationError] = useState("");
  const { busy, error, run } = useAsyncAction();

  // The vault owner may have filled some of this in when making the account.
  // What the owner set is shown for the holder to check and correct, once, and
  // never over something the holder has already typed. A new account's zone is
  // UTC until someone chooses one, so only another zone is the owner's choice.
  // Identities the owner added go into the rows themselves, as many as the card
  // has room for; any beyond that are left as they are, for Settings.
  const { profile } = useAccountProfile();
  const seeded = useRef(false);
  const [seededRows, setSeededRows] = useState<HandleInput[]>([]);
  const [hiddenIdentities, setHiddenIdentities] = useState(0);
  useEffect(() => {
    if (!profile || seeded.current) return;
    seeded.current = true;
    if (profile.preferred_name) setDisplayName((typed) => typed || (profile.preferred_name ?? ""));
    if (profile.time_zone !== DEFAULT_TIME_ZONE) setTimeZone(profile.time_zone);

    const existing = [
      ...profile.phones.map((phone) => newHandleRow(phone, "phone")),
      ...profile.emails.map((email) => newHandleRow(email, "email")),
    ];
    if (existing.length === 0) return;
    const shown = existing.slice(0, MAX_ACCOUNT_ROWS);
    setHiddenIdentities(existing.length - shown.length);
    setSeededRows(shown);
    setHandles((rows) => (rows.some((row) => row.handle.trim()) ? rows : shown));
  }, [profile]);

  const blinkTimer = useRef<number | null>(null);
  const lastAsked = useRef<{ message: string; at: number }>({ message: "", at: 0 });
  useEffect(() => {
    return () => {
      if (blinkTimer.current !== null) clearTimeout(blinkTimer.current);
    };
  }, []);

  /**
   * Put `message` on the error line, blanking the line first when a separate
   * look produced the same words that are already sitting there.
   *
   * Re-checking a field that is still wrong otherwise changes nothing on
   * screen, which reads as the check not having happened at all. Taking the
   * message away and bringing it back a moment later is what makes the second
   * look visible. The line keeps its height throughout, so nothing shifts.
   */
  const showValidationError = (message: string) => {
    if (blinkTimer.current !== null) {
      clearTimeout(blinkTimer.current);
      blinkTimer.current = null;
    }

    const now = Date.now();
    const previous = lastAsked.current;
    lastAsked.current = { message, at: now };

    const isSecondLook =
      Boolean(message) && message === previous.message && now - previous.at > SAME_GESTURE_MS;
    if (!isSecondLook) {
      setValidationError(message);
      return;
    }

    setValidationError("");
    blinkTimer.current = window.setTimeout(() => {
      blinkTimer.current = null;
      setValidationError(message);
    }, REPEATED_ERROR_BLINK_MS);
  };

  /**
   * Check every filled-in row, mark the ones that do not read as a usable
   * account, and report the topmost reason. Returns whether the list is usable.
   * Empty rows are left alone — they are rows not filled in yet, not mistakes.
   */
  const revalidate = (rows: HandleInput[]) => {
    const errors = rowErrors(rows);
    setInvalidIds(rows.filter((row) => errors.has(row.id)).map((row) => row.id));
    const firstBad = rows.find((row) => errors.has(row.id));
    showValidationError(firstBad ? (errors.get(firstBad.id) ?? "") : "");
    return errors.size === 0;
  };

  const addHandle = () => {
    if (handles.length >= MAX_ACCOUNT_ROWS) return;
    // A new empty row while one above it is wrong just buries the mistake, so
    // the list does not grow until what is already in it holds up.
    if (!revalidate(handles)) return;
    setHandles([...handles, newHandleRow()]);
  };

  const updateHandle = (index: number, field: "handle" | "service", value: string) => {
    const next = [...handles];
    if (field === "service") {
      const service = parseSelectKey(value, HANDLE_SERVICES);
      if (!service) return;
      next[index] = { ...next[index], service };
    } else {
      next[index] = { ...next[index], handle: value };
    }
    setHandles(next);

    // A row being edited stops reading as wrong straight away rather than
    // staying red under the cursor; leaving the field checks it again.
    const remaining = invalidIds.filter((id) => id !== next[index].id);
    if (remaining.length !== invalidIds.length) setInvalidIds(remaining);
    // `showValidationError` rather than the setter, so a blink waiting to put
    // the old message back does not fire over a field being corrected.
    if (remaining.length === 0) showValidationError("");
  };

  const removeHandle = (index: number) => {
    if (handles.length === 1) return;
    // Removing a row is itself a way to fix the list, so the row goes first and
    // what is left is judged after.
    const next = handles.filter((_, i) => i !== index);
    setHandles(next);
    revalidate(next);
  };

  const handleSubmit = () => {
    if (!revalidate(handles)) return;
    void run(async () => {
      if (!token || !accountId) {
        throw new Error("Not logged in");
      }
      // An identity the owner added that is no longer in its row, edited or
      // removed, is unlinked; the vault unlinks before it links, so an edit is
      // the old value out and the new one in. Rows left as they were send nothing.
      const filled = handles.filter((h) => h.handle.trim());
      await updateAccountProfile({
        preferred_name: displayName.trim(),
        time_zone: timeZone,
        identities: filled
          .filter((h) => !seededRows.some((original) => sameIdentity(h, original)))
          .map((h) => ({ address: h.handle.trim(), service: h.service })),
        remove_identities: seededRows
          .filter((original) => !filled.some((h) => sameIdentity(h, original)))
          .map(({ handle, service }) => ({ address: handle, service })),
      });
      // Log in again so "needs setup" is recomputed from the saved profile.
      await login(serverUrl, token, accountId);
    });
  };

  // Identities beyond the rows shown stay on the account, so they count too.
  const canSubmit =
    Boolean(displayName.trim()) && (hiddenIdentities > 0 || handles.some((h) => h.handle.trim()));

  // The empty row at the bottom of the list is already the place to put the
  // next account, so adding another one on top of it would only produce a
  // second blank. The control wakes up once that row holds something.
  const canAddHandle = Boolean(handles[handles.length - 1]?.handle.trim());

  return (
    <div className={pageCenter}>
      <div className={authCard}>
        <div className={authCardBody}>
          <h1 className={`${authTitle} !mb-2`}>Profile Setup</h1>
          <p className="mt-0 mb-4 text-[0.875rem] text-muted">
            So we can match imported messages to you.
          </p>

          <TextField
            label="Display Name"
            value={displayName}
            onChange={setDisplayName}
            placeholder="Your name"
          />

          <TimeZoneField
            label="Time Zone"
            value={timeZone}
            onChange={setTimeZone}
            className="mt-4"
          />

          <div className="mt-4 mb-2 block text-[0.875rem] font-medium text-text">Your Accounts</div>

          {handles.map((h, i) => {
            const invalid = invalidIds.includes(h.id);
            return (
              <div key={h.id} className="mb-2 flex items-center gap-2">
                <Select
                  selectedKey={h.service}
                  onSelectionChange={(k) => {
                    const service = parseSelectKey(k, HANDLE_SERVICES);
                    if (service) updateHandle(i, "service", service);
                  }}
                  className="w-[140px] shrink-0"
                  aria-label={`Account ${i + 1} type`}
                >
                  {HANDLE_SERVICE_OPTIONS.map((s) => (
                    <ListBoxItem key={s.value} id={s.value} className={selectItemClassName}>
                      {s.label}
                    </ListBoxItem>
                  ))}
                </Select>
                <TextField
                  value={h.handle}
                  onChange={(v) => updateHandle(i, "handle", v)}
                  onBlur={() => revalidate(handles)}
                  leadingIcon={serviceIcon(h.service)}
                  placeholder={handlePlaceholder(h.service)}
                  className="min-w-0 flex-1"
                  inputClassName={invalid ? "!border-danger" : undefined}
                  isInvalid={invalid}
                  aria-label={`Account ${i + 1} value`}
                />
                {handles.length > 1 ? (
                  <Button
                    variant="ghostDanger"
                    size="icon"
                    onPress={() => removeHandle(i)}
                    aria-label={`Remove account ${i + 1}`}
                  >
                    ×
                  </Button>
                ) : null}
              </div>
            );
          })}

          {handles.length < MAX_ACCOUNT_ROWS ? (
            <div className="mt-3 flex justify-end">
              <Button variant="secondary" size="sm" onPress={addHandle} disabled={!canAddHandle}>
                + Add account
              </Button>
            </div>
          ) : (
            <p className="mt-1.5 text-right text-[0.75rem] text-muted">
              {hiddenIdentities > 0
                ? `${hiddenIdentities} more ${hiddenIdentities === 1 ? "is" : "are"} in Settings.`
                : "Add the rest in Settings after setup."}
            </p>
          )}
        </div>

        <div className={authCardFooter}>
          {/* A value that does not read as an account is reported in the same
              place as anything the server sends back, so there is one line on
              this card that carries what is wrong. */}
          <AuthErrorFooter error={validationError || error} />
          {/* One row: the way back on the left, the way on at half width on
              the right, matching the button on the card before this one. */}
          <div className="mt-6 flex items-center justify-between gap-3">
            <AuthBackButton label="Back to login" onClick={logout} />
            <AuthSubmitButton
              onClick={handleSubmit}
              disabled={!canSubmit || busy}
              className="w-1/2"
            >
              {busy ? "Saving…" : "Continue to vault"}
            </AuthSubmitButton>
          </div>
        </div>
      </div>
    </div>
  );
}
