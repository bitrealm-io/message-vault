import { Navigate, Outlet } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useIsVaultOwner } from "../lib/useIsVaultOwner";
import { useNeedsProfileSetup } from "../lib/useNeedsProfileSetup";

/**
 * Layout route: renders child routes via <Outlet /> when the account may use
 * the app, and otherwise sends it to the one screen it still owes.
 */
export function AuthGuard() {
  const { isAuthenticated } = useAuth();
  const { isOwner } = useIsVaultOwner();
  const { needsSetup, loading } = useNeedsProfileSetup();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  // The profile is fetched during login, so this is over before it is seen.
  // Rendering the app first and redirecting after would flash a screen this
  // account is not finished earning.
  if (loading) {
    return null;
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
