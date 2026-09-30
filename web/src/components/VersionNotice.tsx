import { APP_BUILD } from "../lib/build";
import { productVersionOf, productVersionsDiffer } from "../lib/buildFormat";
import { useServerInfo } from "../lib/useServerInfo";

/**
 * Says so when this app and its server come from different releases. It blocks
 * nothing: the server serves every request whatever the versions are, and this
 * line is how a person learns why a screen might not work. Only the Product
 * Version is compared, so a dev build from another commit shows nothing.
 */
export default function VersionNotice() {
  const { data } = useServerInfo();
  if (!data || !productVersionsDiffer(data.version, APP_BUILD)) return null;

  return (
    <p
      role="status"
      className="m-0 shrink-0 border-b border-border bg-elevated px-3 py-1.5 text-center text-[0.75rem] text-muted"
    >
      The server is {productVersionOf(data.version)}. This app is {productVersionOf(APP_BUILD)}.
    </p>
  );
}
