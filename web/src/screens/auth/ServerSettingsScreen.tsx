import Button from "../../components/Button";
import PlainButton from "../../components/PlainButton";
import TextField from "../../components/TextField";
import { DEFAULT_TAURI_SERVER_URL } from "../../lib/authGuards";
import { accentLink, authLabel, authScreenTitle } from "../../lib/uiStyles";
import ServerStatus, { type ServerConnection } from "./ServerStatus";

export interface ServerSettingsScreenProps {
  /** Address being typed. */
  draft: string;
  /**
   * What to report under Connection Status. Whatever it says, it says about
   * the address in the field and nothing else — the caller is the one that
   * knows whether its own connection was made to that address or to a
   * different one it is still holding behind this screen.
   */
  status: ServerConnection;
  /**
   * Whether the address in the field is one there is anything to apply.
   * The caller decides: it is the one that knows which address the card is
   * already connected to, and re-applying that one is a change that is not a
   * change. Test is unaffected — re-probing the current address is a real
   * answer to a real question.
   */
  canSubmit: boolean;
  onDraftChange: (value: string) => void;
  onTest: () => void;
  onCancel: () => void;
  onSubmit: () => void;
  /**
   * Go back to the Message Crate the desktop app starts for itself. Given
   * only in the desktop app, and only while the address in use is another one:
   * it is how a person returns from a Message Crate elsewhere.
   */
  onUseOwn?: () => void;
}

/**
 * Where the server address is chosen. It takes over the auth card rather than
 * opening a dialog, so the frame never changes size, and it answers the one
 * question the address raises — will this work? — in place, before you commit
 * to it.
 */
export default function ServerSettingsScreen({
  draft,
  status,
  canSubmit,
  onDraftChange,
  onTest,
  onCancel,
  onSubmit,
  onUseOwn,
}: ServerSettingsScreenProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <h1 className={`${authScreenTitle} mb-6`}>Server Address</h1>

      <div className="flex items-end gap-2">
        <TextField
          label="Address"
          value={draft}
          onChange={onDraftChange}
          onKeyDown={(e) => e.key === "Enter" && onTest()}
          placeholder={DEFAULT_TAURI_SERVER_URL}
          className="min-w-0 flex-1"
          spellCheck="false"
        />
        <Button variant="secondary" onPress={onTest} className="shrink-0">
          Test
        </Button>
      </div>

      <div className={`mt-3.5 ${authLabel}`}>Connection Status</div>
      {/* 13px lines the word up with the first character inside the field. */}
      <ServerStatus state={status} className="pl-[13px]" />

      <div className="mt-6 grid grid-cols-2 gap-2.5">
        <Button variant="secondary" onPress={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onPress={onSubmit} disabled={!canSubmit}>
          Use this address
        </Button>
      </div>

      {onUseOwn ? (
        <div className="mt-4 text-center">
          <PlainButton className={accentLink} onPress={onUseOwn}>
            Use the Message Crate on this computer
          </PlainButton>
        </div>
      ) : null}
    </div>
  );
}
