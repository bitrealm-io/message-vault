import { Navigate, Outlet } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useIsOwner } from "../lib/useIsOwner";
import { useNeedsProfileSetup } from "../lib/useNeedsProfileSetup";
import Button from "./Button";

/**
 * Layout route: renders child routes via <Outlet /> when the account may use
 * the app, and otherwise sends it to the one screen it still owes.
 */
export function AuthGuard() {
  const { isAuthenticated } = useAuth();
  const { isOwner, error: ownerError } = useIsOwner();
  const { needsSetup, loading, error: setupError } = useNeedsProfileSetup();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  // The profile is fetched during login, so this is over before it is seen.
  // Rendering the app first and redirecting after would flash a screen this
  // account is not finished earning.
  if (loading) {
    return null;
  }

  // With no profile, neither the owner nor an account that owes its setup can
  // be told apart from an account that owes nothing, so none of the screens
  // below may render.
  const error = ownerError || setupError;
  if (error) {
    return <ProfileLoadFailed error={error} />;
  }

  // The owner holds no messages, so every route under this guard is empty for
  // them. Owner Home is the whole of what they have.
  if (isOwner) {
    return <Navigate to="/owner" replace />;
  }

  if (needsSetup) {
    return <Navigate to="/onboarding" replace />;
  }

  return <Outlet />;
}

/** In place of the app while the account's profile cannot be loaded. */
function ProfileLoadFailed({ error }: { error: string }) {
  const { retry } = useAccountProfile();
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 bg-bg p-6 font-sans text-text">
      <div className="max-w-md text-center" role="alert">
        <p className="m-0 font-semibold text-[0.938rem]">Your profile could not be loaded.</p>
        <p className="m-0 mt-1 text-[0.813rem] text-muted">{error}</p>
      </div>
      <Button variant="primary" onPress={retry}>
        Try again
      </Button>
    </div>
  );
}
