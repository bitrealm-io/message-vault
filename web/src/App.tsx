import { lazy, Suspense } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import AppLayout from "./components/AppLayout";
import { AuthGuard } from "./components/AuthGuard";
import ImportExportRoute from "./components/ImportExportRoute";
import MessageRoute from "./components/MessageRoute";
import { useMouseHistoryNavigation } from "./hooks/useMouseHistoryNavigation";
import { AuthProvider, useAuth } from "./lib/auth";
import { ThemeProvider } from "./lib/ThemeProvider";
import { TimeZoneProvider } from "./lib/TimeZoneProvider";
import { useIsOwner } from "./lib/useIsOwner";
import { useNeedsProfileSetup } from "./lib/useNeedsProfileSetup";
import LoginScreen from "./screens/LoginScreen";
import OnboardingScreen from "./screens/OnboardingScreen";
import OwnerHome from "./screens/OwnerHome";

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

function AppRoutes() {
  const { isAuthenticated } = useAuth();
  const { isOwner } = useIsOwner();
  const { needsSetup: needsOnboarding } = useNeedsProfileSetup();
  useMouseHistoryNavigation();

  // Where a logged-in visitor to the login screen should go next. Same order
  // the AuthGuard uses.
  const loggedInDestination = (
    <Navigate to={isOwner ? "/owner" : needsOnboarding ? "/onboarding" : "/"} replace />
  );

  return (
    <Routes>
      {/* Public routes — an authenticated visitor goes to loggedInDestination */}
      <Route path="/login" element={isAuthenticated ? loggedInDestination : <LoginScreen />} />
      {/* Registration is now the second tab of the login card, not its own screen. */}
      <Route path="/register" element={<Navigate to="/login" replace />} />
      {/* Owner Home, outside the AuthGuard's message shell: the owner holds
          no messages, so none of what that shell frames exists for them. */}
      <Route
        path="/owner/:section?/:accountId?"
        element={isAuthenticated && isOwner ? <OwnerHome /> : <Navigate to="/" replace />}
      />
      <Route
        path="/onboarding"
        element={
          isAuthenticated && needsOnboarding ? <OnboardingScreen /> : <Navigate to="/" replace />
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
              <ImportExportRoute feature="import">
                <ImportScreen />
              </ImportExportRoute>
            }
          />
          <Route
            path="export"
            element={
              <ImportExportRoute feature="export">
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
