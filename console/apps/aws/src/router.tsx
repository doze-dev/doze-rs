import { createRootRoute, createRoute, createRouter, lazyRouteComponent } from "@tanstack/react-router";
import { basePathFromMeta } from "@doze/ui/lib/api";
import { unlessLeaving } from "@doze/ui/lib/leaving";
import { parseSearch, stringifySearch } from "@doze/ui/lib/search-params";
import { HomePage } from "./pages/home/home";
import { NotFoundPage } from "./pages/not-found";
// Live's address (the drawer of a call, the view, the filters): a small module of its own, so the router does not pull Live in.
import { liveSearch } from "./pages/live/search";
import { labSearch } from "./pages/lab/search";
import { indexSearch, oneSearch, resourceSearch } from "./pages/resources/search";
import { Shell } from "./shell/shell";

const root = createRootRoute({ component: Shell, notFoundComponent: NotFoundPage });

const home = createRoute({ getParentRoute: () => root, path: "/", component: HomePage });
// Live (the table, the stream reader, the drawer) is its own chunk, fetched when the space is first opened. A load that the
// page's leaving cut short is not an error (lib/leaving).
const live = createRoute({ getParentRoute: () => root, path: "/live", validateSearch: liveSearch, component: lazyRouteComponent(unlessLeaving(() => import("./pages/live/live")), "LivePage") });
// Resources is a chunk of its own too. `/resources/<service>/<x>` is a kind's list, or what Live's "Open" links to
// (`/resources/sqs/orders?m=QueueUrl`): the space resolves which.
const resourcesChunk = () => import("./pages/resources/routes");
const resources = createRoute({ getParentRoute: () => root, path: "/resources", validateSearch: indexSearch, component: lazyRouteComponent(unlessLeaving(() => import("./pages/resources/index-page")), "ResourcesPage") });
const service = createRoute({ getParentRoute: () => root, path: "/resources/$service", component: lazyRouteComponent(unlessLeaving(resourcesChunk), "ServicePage") });
const second = createRoute({ getParentRoute: () => root, path: "/resources/$service/$second", validateSearch: resourceSearch, component: lazyRouteComponent(unlessLeaving(resourcesChunk), "SecondPage") });
const resource = createRoute({ getParentRoute: () => root, path: "/resources/$service/$kind/$name", validateSearch: oneSearch, component: lazyRouteComponent(unlessLeaving(resourcesChunk), "OnePage") });
// Connect, Access, Lab and Settings: each a chunk of its own, fetched when first opened.
const connect = createRoute({ getParentRoute: () => root, path: "/connect", component: lazyRouteComponent(unlessLeaving(() => import("./pages/home/connect")), "ConnectPage") });
const access = createRoute({ getParentRoute: () => root, path: "/access", component: lazyRouteComponent(unlessLeaving(() => import("./pages/access/access")), "AccessPage") });
const lab = createRoute({ getParentRoute: () => root, path: "/lab", validateSearch: labSearch, component: lazyRouteComponent(unlessLeaving(() => import("./pages/lab/lab")), "LabPage") });
const settings = createRoute({ getParentRoute: () => root, path: "/settings", component: lazyRouteComponent(unlessLeaving(() => import("./pages/settings/settings")), "SettingsPage") });

export const router = createRouter({
  routeTree: root.addChildren([home, connect, live, resources, service, second, resource, access, lab, settings]),
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
