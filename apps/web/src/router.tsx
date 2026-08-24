import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent
} from "@tanstack/react-router";
import { AppShell } from "./components/app-shell";

const rootRoute = createRootRoute({ component: AppShell });
const hostsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: lazyRouteComponent(() => import("./screens/overview-screens"), "HostsScreen") });
const sessionsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/sessions", component: lazyRouteComponent(() => import("./screens/overview-screens"), "SessionsScreen") });
const sessionRoute = createRoute({ getParentRoute: () => rootRoute, path: "/sessions/$sessionId", component: lazyRouteComponent(() => import("./screens/session-screen"), "SessionScreen") });
const workspacesRoute = createRoute({ getParentRoute: () => rootRoute, path: "/workspaces", component: lazyRouteComponent(() => import("./screens/overview-screens"), "WorkspacesScreen") });
const activityRoute = createRoute({ getParentRoute: () => rootRoute, path: "/activity", component: lazyRouteComponent(() => import("./screens/overview-screens"), "ActivityScreen") });
const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: lazyRouteComponent(() => import("./screens/settings-screen"), "SettingsScreen") });
const setupRoute = createRoute({ getParentRoute: () => rootRoute, path: "/setup", component: lazyRouteComponent(() => import("./screens/setup-screen"), "SetupScreen") });

const routeTree = rootRoute.addChildren([
  hostsRoute,
  sessionsRoute,
  sessionRoute,
  workspacesRoute,
  activityRoute,
  settingsRoute,
  setupRoute
]);

export const router = createRouter({ routeTree, defaultPreload: "intent" });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
