import { useEffect, useState } from "react";
import Button from "./Button";
import ModalShell from "./ModalShell";

function CopyIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

export default function ApiTokenRevealDialog({
  open,
  label,
  token,
  onClose,
}: {
  open: boolean;
  label: string;
  token: string;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (open) setCopied(false);
  }, [open]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(token);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      label="API Token created"
      maxWidth="32rem"
    >
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        className="absolute top-3 right-3 cursor-pointer border-none bg-transparent text-[1.25rem] leading-none text-muted"
      >
        ×
      </button>

      <h3 className="mb-3 pr-6 text-center text-[1.125rem] font-medium text-text">
        API Token created
      </h3>
      <p className="mb-4 text-center text-[0.875rem] leading-relaxed text-muted">
        Your new API Token <strong className="font-medium text-text">{label}</strong> has been
        created. Copy this token now as it won&apos;t be shown again.
      </p>

      <div className="mb-3 flex items-stretch gap-2">
        <div className="min-w-0 flex-1 overflow-hidden rounded-xl border border-border bg-bg px-3 py-2.5 font-mono text-[0.813rem] text-text">
          <span className="block truncate" title={token}>
            {token}
          </span>
        </div>
        <Button
          variant="secondary"
          onClick={() => void copy()}
          className="!inline-flex !shrink-0 !items-center !gap-1.5 !rounded-xl !px-3 !py-2 !text-[0.813rem]"
        >
          <CopyIcon />
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>

      <p className="mb-5 text-[0.75rem] leading-relaxed text-muted">
        For security reasons, this token is only displayed once and cannot be retrieved later. If
        you lose it, you&apos;ll need to create a new one.
      </p>

      <div className="flex justify-end">
        <Button
          variant="secondary"
          onClick={onClose}
          className="!rounded-xl !px-4 !py-2 !text-[0.875rem]"
        >
          Done
        </Button>
      </div>
    </ModalShell>
  );
}
