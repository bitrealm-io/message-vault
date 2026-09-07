import Button from "../../components/Button";
import TextField from "../../components/TextField";
import { authLabel, authScreenTitle } from "../../lib/uiStyles";
import VaultStatus, { type VaultConnection } from "./VaultStatus";

export interface VaultSettingsScreenProps {
  /** Address being typed. */
  draft: string;
  /**
   * What to report under Connection Status. Whatever it says, it says about
   * the address in the field and nothing else — the caller is the one that
   * knows whether its own connection was made to that address or to a
   * different one it is still holding behind this screen.
   */
  status: VaultConnection;
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
}

/**
 * Where the vault address is chosen. It takes over the auth card rather than
 * opening a dialog, so the frame never changes size, and it answers the one
 * question the address raises — will this work? — in place, before you commit
 * to it.
 */
export default function VaultSettingsScreen({
  draft,
  status,
  canSubmit,
  onDraftChange,
  onTest,
  onCancel,
  onSubmit,
}: VaultSettingsScreenProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <h1 className={`${authScreenTitle} mb-6`}>Message Vault Settings</h1>

      <div className="flex items-end gap-2">
        <TextField
          label="Address"
          value={draft}
          onChange={onDraftChange}
          onKeyDown={(e) => e.key === "Enter" && onTest()}
          placeholder="https://vault.example.com"
          className="min-w-0 flex-1"
          spellCheck="false"
        />
        <Button variant="secondary" onPress={onTest} className="shrink-0">
          Test
        </Button>
      </div>

      <div className={`mt-3.5 ${authLabel}`}>Connection Status</div>
      {/* 13px lines the word up with the first character inside the field. */}
      <VaultStatus state={status} className="pl-[13px]" />

      <div className="mt-6 grid grid-cols-2 gap-2.5">
        <Button variant="secondary" onPress={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onPress={onSubmit} disabled={!canSubmit}>
          Change vault address
        </Button>
      </div>
    </div>
  );
}
