import { createRootRoute, createRoute, createRouter, lazyRouteComponent } from "@tanstack/react-router";
import { basePathFromMeta } from "@doze/ui/lib/api";
import { unlessLeaving } from "@doze/ui/lib/leaving";
import { parseSearch, stringifySearch } from "@doze/ui/lib/search-params";
import { HomePage } from "./pages/home";
import { NotFoundPage } from "./pages/not-found";
import { AccessPage, LabPage, ResourcesPage, SettingsPage } from "./pages/soon";
// Live's address (the drawer of a call, the view, the filters): a small module of its own, so the router does not pull Live in.
import { liveSearch } from "./pages/live/search";
import { Shell } from "./shell/shell";

const root = createRootRoute({ component: Shell, notFoundComponent: NotFoundPage });

const home = createRoute({ getParentRoute: () => root, path: "/", component: HomePage });
// Live (the table, the stream reader, the drawer) is its own chunk, fetched when the space is first opened. A load that the
// page's leaving cut short is not an error (lib/leaving).
const live = createRoute({ getParentRoute: () => root, path: "/live", validateSearch: liveSearch, component: lazyRouteComponent(unlessLeaving(() => import("./pages/live/live")), "LivePage") });
const resources = createRoute({ getParentRoute: () => root, path: "/resources", component: ResourcesPage });
// What Live's "Open" links to (`/resources/sqs/orders`). The page itself comes with the Resources space; until then it says so.
const resource = createRoute({ getParentRoute: () => root, path: "/resources/$service/$name", component: ResourcesPage });
const access = createRoute({ getParentRoute: () => root, path: "/access", component: AccessPage });
const lab = createRoute({ getParentRoute: () => root, path: "/lab", component: LabPage });
const settings = createRoute({ getParentRoute: () => root, path: "/settings", component: SettingsPage });

export const router = createRouter({
  routeTree: root.addChildren([home, live, resources, resource, access, lab, settings]),
  basepath: basePathFromMeta() || "/",
  defaultPreload: false,
  scrollRestoration: true,
  defaultStructuralSharing: true,
  parseSearch,
  stringifySearch,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
