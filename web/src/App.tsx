import { lazy, type ReactNode, Suspense } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import AppLayout from "./components/AppLayout";
import { AuthGuard } from "./components/AuthGuard";
import MessageRoute from "./components/MessageRoute";
import { useMouseHistoryNavigation } from "./hooks/useMouseHistoryNavigation";
import { AuthProvider, useAuth } from "./lib/auth";
import { canUseImportExportWithProfile } from "./lib/desktopFeatures";
import { ThemeProvider } from "./lib/ThemeProvider";
import { TimeZoneProvider } from "./lib/TimeZoneProvider";
import { isTauri } from "./lib/tauri-check";
import { useAccountProfile } from "./lib/useAccountProfile";
import { useIsVaultOwner } from "./lib/useIsVaultOwner";
import { useMustChangePassword } from "./lib/useMustChangePassword";
import { useNeedsProfileSetup } from "./lib/useNeedsProfileSetup";
import LoginScreen from "./screens/LoginScreen";
import OnboardingScreen from "./screens/OnboardingScreen";
import OwnerConsole from "./screens/OwnerConsole";
import SetPasswordScreen from "./screens/SetPasswordScreen";

/**
 * Import and export only ever run in the desktop app, so their code — the
 * importer forms, the job runner and the Tauri bridge behind them — is split out
 * and never downloaded by a browser visiting the website build.
 */
const ImportScreen = lazy(() => import("./screens/ImportScreen"));
const ExportScreen = lazy(() => import("./screens/ExportScreen"));

/** Settings and trash are their own routes and are not on the first paint path. */
const SettingsScreen = lazy(() => import("./screens/SettingsScreen"));
const TrashScreen = lazy(() => import("./screens/TrashScreen"));

/** Import and export stay on the desktop app. */
function ImportExportRoute({ children }: { children: ReactNode }) {
  const { profile, loading } = useAccountProfile();
  if (!isTauri()) {
    return <Navigate to="/" replace />;
  }
  if (loading) {
    return null;
  }
  if (profile == null || !canUseImportExportWithProfile(true, profile)) {
    return <Navigate to="/" replace />;
  }
  // The chunk only starts loading once the route is allowed, so the redirect
  // paths above never pay for it.
  return <Suspense fallback={null}>{children}</Suspense>;
}

function AppRoutes() {
  const { isAuthenticated } = useAuth();
  const { mustChange: mustChangePassword } = useMustChangePassword();
  const { isOwner } = useIsVaultOwner();
  const { needsSetup: needsOnboarding } = useNeedsProfileSetup();
  useMouseHistoryNavigation();

  // Where a signed-in visitor to the login screen should go next. Same order
  // of urgency the AuthGuard uses: the password before the profile.
  const signedInDestination = (
    <Navigate
      to={
        mustChangePassword
          ? "/set-password"
          : isOwner
            ? "/admin"
            : needsOnboarding
              ? "/onboarding"
              : "/"
      }
      replace
    />
  );

  return (
    <Routes>
      {/* Public routes — redirect to / if already authenticated */}
      <Route path="/login" element={isAuthenticated ? signedInDestination : <LoginScreen />} />
      {/* Registration is now the second tab of the login card, not its own screen. */}
      <Route path="/register" element={<Navigate to="/login" replace />} />
      {/* The vault owner's console, outside the AuthGuard's message shell:
          the owner holds no messages, so none of what that shell frames
          exists for them. */}
      <Route
        path="/admin"
        element={isAuthenticated && isOwner ? <OwnerConsole /> : <Navigate to="/" replace />}
      />
      <Route
        path="/set-password"
        element={
          isAuthenticated && mustChangePassword ? (
            <SetPasswordScreen />
          ) : (
            <Navigate to="/" replace />
          )
        }
      />
      <Route
        path="/onboarding"
        element={
          isAuthenticated && mustChangePassword ? (
            <Navigate to="/set-password" replace />
          ) : isAuthenticated && needsOnboarding ? (
            <OnboardingScreen />
          ) : (
            <Navigate to="/" replace />
          )
        }
      />

      {/* Protected routes — AuthGuard redirects to /login or /onboarding */}
      <Route element={<AuthGuard />}>
        <Route
          element={
            <TimeZoneProvider>
              <AppLayout />
            </TimeZoneProvider>
          }
        >
          <Route index element={null} />
          <Route path="contacts" element={null} />
          <Route path="group/:slug" element={null} />
          <Route path="no-group" element={null} />
          <Route path="unknown" element={null} />
          <Route path="tag/:slug" element={null} />
          <Route path="no-tag" element={null} />
          <Route
            path="trash"
            element={
              <Suspense fallback={null}>
                <TrashScreen />
              </Suspense>
            }
          />
          <Route
            path="import"
            element={
              <ImportExportRoute>
                <ImportScreen />
              </ImportExportRoute>
            }
          />
          <Route
            path="export"
            element={
              <ImportExportRoute>
                <ExportScreen />
              </ImportExportRoute>
            }
          />
          <Route
            path="settings"
            element={
              <Suspense fallback={null}>
                <SettingsScreen />
              </Suspense>
            }
          />
          <Route path="messages/:conversationId" element={<MessageRoute />} />
        </Route>
      </Route>

      {/* Catch-all */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <AppRoutes />
      </AuthProvider>
    </ThemeProvider>
  );
}
