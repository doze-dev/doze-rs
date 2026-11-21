import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { shortText } from "@doze/ui/lib/search";
import { parseSearch, stringifySearch } from "@doze/ui/lib/search-params";
import { ActionsPage } from "./pages/actions";
import { ChartsPage } from "./pages/charts";
import { CreatePage } from "./pages/create";
import { FieldsPage } from "./pages/fields";
import { ListsPage } from "./pages/lists";
import { ResourcePageGallery } from "./pages/resource";
import { DataPage } from "./pages/data";
import { FormsPage } from "./pages/forms";
import { FoundationsPage } from "./pages/foundations";
import { FramePage } from "./pages/frame";
import { OverlaysPage } from "./pages/overlays";
import { StatesPage } from "./pages/states";
import { Shell } from "./shell";

const root = createRootRoute({ component: Shell });
const page = <const P extends string>(path: P, component: () => React.ReactNode) => createRoute({ getParentRoute: () => root, path, component });

/** The Overlays page has one overlay with its own address: the record Sheet (`?record=r7`), as a console's drawers have. */
export const overlaysRoute = createRoute({
  getParentRoute: () => root,
  path: "/overlays",
  component: OverlaysPage,
  validateSearch: (s: Record<string, unknown>): { record?: string } => ({ record: shortText(s.record, 16) }),
});

const tree = root.addChildren([page("/", FoundationsPage), page("/actions", ActionsPage), page("/forms", FormsPage), overlaysRoute, page("/data", DataPage), page("/states", StatesPage), page("/frame", FramePage), page("/create", CreatePage), page("/fields", FieldsPage), page("/resource", ResourcePageGallery), page("/lists", ListsPage), page("/charts", ChartsPage)]);

export const router = createRouter({ routeTree: tree, parseSearch, stringifySearch, scrollRestoration: true });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
