import { Navigate, Outlet } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useIsVaultOwner } from "../lib/useIsVaultOwner";
import { useMustChangePassword } from "../lib/useMustChangePassword";
import { useNeedsProfileSetup } from "../lib/useNeedsProfileSetup";

/**
 * Layout route: renders child routes via <Outlet /> when the account may use
 * the app, and otherwise sends it to the one screen it still owes.
 *
 * The order is the order of urgency. An account still using the password the
 * vault owner chose is using a credential someone else knows, which matters
 * more than not having named itself yet.
 */
export function AuthGuard() {
  const { isAuthenticated } = useAuth();
  const { mustChange, loading } = useMustChangePassword();
  const { isOwner } = useIsVaultOwner();
  const { needsSetup } = useNeedsProfileSetup();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  // The profile is fetched during sign-in, so this is over before it is seen.
  // Rendering the app first and redirecting after would flash a screen this
  // account is not finished earning.
  if (loading) {
    return null;
  }

  if (mustChange) {
    return <Navigate to="/set-password" replace />;
  }

  // The owner holds no messages, so every route under this guard is empty for
  // them. Their console is the whole of what they have.
  if (isOwner) {
    return <Navigate to="/admin" replace />;
  }

  if (needsSetup) {
    return <Navigate to="/onboarding" replace />;
  }

  return <Outlet />;
}
