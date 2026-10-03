import { type ReactNode, useState } from "react";
import { openPathInExplorer } from "../lib/openPath";
import PlainButton from "./PlainButton";

type OpenPathButtonProps = {
  path: string;
  children: ReactNode;
  className?: string;
  title?: string;
};

/** Text button that opens a file or directory with the OS default handler. */
export default function OpenPathButton({ path, children, className, title }: OpenPathButtonProps) {
  const [error, setError] = useState<string | null>(null);

  // React Aria's press does not reach an ancestor's click handler, so the row around this button stays put.
  async function onPress(): Promise<void> {
    setError(null);
    try {
      await openPathInExplorer(path);
    } catch (caught) {
      const message = caught instanceof Error ? caught.message : String(caught);
      setError(message || "Could not open path");
      console.error("Failed to open path", caught);
    }
  }

  return (
    <span className="inline-flex max-w-full flex-col items-start">
      <PlainButton onPress={() => void onPress()} title={title ?? path} className={className}>
        {children}
      </PlainButton>
      {error ? (
        <span className="mt-0.5 text-[0.75rem] text-danger" role="alert">
          {error}
        </span>
      ) : null}
    </span>
  );
}
