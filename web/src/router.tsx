import { createRootRoute, createRoute, createRouter, lazyRouteComponent, Outlet } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { basePath } from "@/api/client";
import { useClosedToNetwork } from "@/api/closed";
import { ClosedToNetworkPage } from "@/pages/closed-to-network";
import { AppSidebar } from "@/components/app/app-sidebar";
import { CommandPalette } from "@/components/app/command-palette";
import { TopBar } from "@/components/app/top-bar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { ConnectPage } from "@/pages/connect";
import { accessSearch, configSearch, connectSearch, createSearch, gallerySearch, labSearch, settingsSearch } from "@/lib/search";
import { useSidebarOpen } from "@/lib/sidebar-state";
import { parseSearch, stringifySearch } from "@/lib/search-params";
import { messagesSearch, partitionSearch, topicSearch, topicsSearch } from "@/lib/topic-search";
import { HomePage } from "@/pages/home";
import { liveSearch } from "@/pages/live/search";
import { NotFoundPage } from "@/pages/not-found";
import { groupSearch, groupsSearch, lagSearch, membersSearch, storySearch } from "@/pages/groups/search";

function Shell() {
  const closed = useClosedToNetwork();
  return closed === null ? <Console /> : <ClosedToNetworkPage said={closed} />;
}

function Console() {
  const [palette, setPalette] = useState(false);
  const [sidebar, setSidebar] = useSidebarOpen();
  // Where the focus goes back when the palette closes: what had it when the palette opened, else the top bar's trigger.
  const opener = useRef<HTMLElement | null>(null);
  const openPalette = (o: boolean) => {
    if (o) {
      const active = document.activeElement as HTMLElement | null;
      opener.current = active && active !== document.body ? active : document.querySelector<HTMLElement>("[data-testid=palette-trigger]");
    }
    setPalette(o);
  };
  return (
    <SidebarProvider className="[--sidebar-width:13rem]" open={sidebar} onOpenChange={setSidebar}>
      <AppSidebar />
      <SidebarInset className="min-w-0 bg-background">
        <TopBar onPalette={() => openPalette(true)} />
        <main id="main" className="mx-auto w-full max-w-[1600px] min-w-0 px-6 pt-6 pb-20 lg:px-10 xl:px-12" tabIndex={-1}>
          <Outlet />
        </main>
        <CommandPalette open={palette} onOpenChange={openPalette} returnFocus={() => (opener.current?.isConnected ? opener.current : document.querySelector<HTMLElement>("[data-testid=palette-trigger]"))} />
      </SidebarInset>
    </SidebarProvider>
  );
}

const root = createRootRoute({ component: Shell, notFoundComponent: NotFoundPage });

const home = createRoute({ getParentRoute: () => root, path: "/", component: HomePage });
const connect = createRoute({
  getParentRoute: () => root,
  path: "/connect",
  validateSearch: connectSearch,
  component: ConnectPage,
});
// The gallery (and react-hook-form and zod with it) is its own chunk: fetched only when someone opens it.
const gallery = createRoute({ getParentRoute: () => root, path: "/gallery", validateSearch: gallerySearch, component: lazyRouteComponent(() => import("@/pages/gallery"), "GalleryPage") });

// Live (R3): the tape, its tail and its Sheets; the page a new client lands on. Their own chunk, fetched when opened.
const live = createRoute({ getParentRoute: () => root, path: "/live", validateSearch: liveSearch, component: lazyRouteComponent(() => import("@/pages/live/live"), "LivePage") });
const firstRequest = createRoute({ getParentRoute: () => root, path: "/connect/first-request", component: lazyRouteComponent(() => import("@/pages/live/first-request"), "FirstRequestPage") });

// R1a: the Topics pages, one lazily loaded chunk (pages/topics/pages.ts). The frame is a layout route; each tab is a child.
const topicsPage = (name: keyof typeof import("@/pages/topics/pages")) => lazyRouteComponent(() => import("@/pages/topics/pages"), name);
const topics = createRoute({ getParentRoute: () => root, path: "/topics", validateSearch: topicsSearch, component: topicsPage("TopicsPage") });
const topic = createRoute({ getParentRoute: () => root, path: "/topics/$topic", validateSearch: topicSearch, component: topicsPage("TopicLayout") });
const topicIndex = createRoute({ getParentRoute: () => topic, path: "/", validateSearch: messagesSearch, component: topicsPage("MessagesPage") });
const topicMessages = createRoute({ getParentRoute: () => topic, path: "/messages", validateSearch: messagesSearch, component: topicsPage("MessagesPage") });
const topicPartitions = createRoute({ getParentRoute: () => topic, path: "/partitions", validateSearch: partitionSearch, component: topicsPage("PartitionsPage") });
const topicLog = createRoute({ getParentRoute: () => topic, path: "/log", validateSearch: partitionSearch, component: topicsPage("LogPage") });
const topicConfig = createRoute({ getParentRoute: () => topic, path: "/config", validateSearch: configSearch, component: topicsPage("TopicConfigPage") });
// R1b: Create topic is a page of three steps (its own chunk, with zod).
const topicsNew = createRoute({ getParentRoute: () => root, path: "/topics/new", validateSearch: createSearch, component: lazyRouteComponent(() => import("@/pages/topics/create"), "CreateTopicPage") });
const topicConsumers = createRoute({ getParentRoute: () => topic, path: "/consumers", component: topicsPage("ConsumersPage") });
const topicProducers = createRoute({ getParentRoute: () => topic, path: "/producers", validateSearch: partitionSearch, component: topicsPage("ProducersPage") });
const topicTxn = createRoute({ getParentRoute: () => topic, path: "/transactions/$txn", component: topicsPage("TxnPage") });
const topicTxnHeld = createRoute({ getParentRoute: () => topic, path: "/transactions/$txn/held", component: topicsPage("TxnHeldPage") });
const topicTree = topic.addChildren([topicIndex, topicMessages, topicPartitions, topicLog, topicConfig, topicConsumers, topicProducers, topicTxn, topicTxnHeld]);
// Groups (R2): the list, and a group whose tabs are routes under it. Each page is its own chunk.
const groups = createRoute({ getParentRoute: () => root, path: "/groups", validateSearch: groupsSearch, component: lazyRouteComponent(() => import("@/pages/groups/list"), "GroupsPage") });
const group = createRoute({ getParentRoute: () => root, path: "/groups/$group", validateSearch: groupSearch, component: lazyRouteComponent(() => import("@/pages/groups/layout"), "GroupLayout") });
const groupTabs = [
  createRoute({ getParentRoute: () => group, path: "/", validateSearch: membersSearch, component: lazyRouteComponent(() => import("@/pages/groups/members"), "GroupMembersPage") }),
  createRoute({ getParentRoute: () => group, path: "lag", validateSearch: lagSearch, component: lazyRouteComponent(() => import("@/pages/groups/lag"), "GroupLagPage") }),
  createRoute({ getParentRoute: () => group, path: "story", validateSearch: storySearch, component: lazyRouteComponent(() => import("@/pages/groups/story"), "GroupStoryPage") }),
  createRoute({ getParentRoute: () => group, path: "wire", component: lazyRouteComponent(() => import("@/pages/groups/wire"), "GroupWirePage") }),
] as const;

// Slice R4: Settings, Access, the Lab: each its own chunk, fetched when first opened.
const settings = createRoute({ getParentRoute: () => root, path: "/settings", validateSearch: settingsSearch, component: lazyRouteComponent(() => import("@/pages/settings"), "SettingsPage") });
const access = createRoute({ getParentRoute: () => root, path: "/access", validateSearch: accessSearch, component: lazyRouteComponent(() => import("@/pages/access"), "AccessPage") });
const lab = createRoute({ getParentRoute: () => root, path: "/lab", validateSearch: labSearch, component: lazyRouteComponent(() => import("@/pages/lab"), "LabPage") });

const tree = root.addChildren([
  home,
  connect,
  gallery,
  live,
  firstRequest,
  topics,
  topicsNew,
  topicTree,
  groups,
  group.addChildren(groupTabs),
  access,
  lab,
  settings,
]);

export const router = createRouter({
  routeTree: tree,
  basepath: basePath || "/",
  defaultPreload: false,
  scrollRestoration: true,
  defaultStructuralSharing: true,
  // Lists as repeated names (`?topic=orders&topic=x`), not JSON; old JSON links still read (lib/search-params.ts).
  parseSearch,
  stringifySearch,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
