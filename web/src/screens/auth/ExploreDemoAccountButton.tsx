import Button from "../../components/Button";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/auth";
import { login as serverLogin } from "../../lib/serverApi";
import { useAsyncAction } from "../../lib/useAsyncAction";

/** The Demo Account's username. It has no password, so this is all it takes. */
const DEMO_USERNAME = "demo";

/**
 * The way into the Demo Account from the login card, shown for as long as the
 * server says the account exists.
 *
 * The Demo Account has no password and can never be given one, so there is
 * nothing to type: the button logs in as it. Without the button a person
 * would have to learn the username from the documentation, and an unclaimed
 * Message Crate shows no login form to type it into. See
 * `docs/adr/0016-the-demo-account-is-fixed-not-configured.md`.
 */
export default function ExploreDemoAccountButton({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const { busy, error, run } = useAsyncAction();

  const explore = () => {
    if (busy || disabled) return;
    void run(async () => {
      const url = serverUrl.trim();
      // The same re-sync `LoginForm` does: the card's connect can leave the
      // API client pointed at an address other than the one shown.
      setBaseUrl(url);
      const res = await serverLogin({ username: DEMO_USERNAME, password: "" });
      await login(url, res.token, res.account_id);
    });
  };

  return (
    <div className="mt-4 flex flex-col items-center">
      <Button
        variant="secondary"
        isDisabled={busy || disabled}
        onPress={explore}
        className="min-w-[50%]"
      >
        {busy ? "Opening…" : "Explore Demo Account"}
      </Button>
      {error ? (
        <p role="alert" className="mt-2 text-center text-[0.813rem] text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
