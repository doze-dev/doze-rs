import { createRootRoute, createRoute, createRouter, lazyRouteComponent, Outlet } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { basePath } from "@/api/client";
import { AppSidebar } from "@/components/app/app-sidebar";
import { CommandPalette } from "@/components/app/command-palette";
import { TopBar } from "@/components/app/top-bar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { ConnectPage } from "@/pages/connect";
import { connectSearch, gallerySearch } from "@/lib/search";
import { messagesSearch, partitionSearch, topicSearch, topicsSearch } from "@/lib/topic-search";
import { HomePage } from "@/pages/home";
import { NotFoundPage, PlannedPage } from "@/pages/planned";

function Shell() {
  const [palette, setPalette] = useState(false);
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
    <SidebarProvider className="[--sidebar-width:13rem]">
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

const planned = <const P extends string>(path: P, title: string, slice: string) =>
  createRoute({ getParentRoute: () => root, path, component: () => <PlannedPage title={title} slice={slice} classicPath={path} /> });

// R1a: the Topics pages, one lazily loaded chunk (pages/topics/pages.ts). The frame is a layout route; each tab is a child.
const topicsPage = (name: keyof typeof import("@/pages/topics/pages")) => lazyRouteComponent(() => import("@/pages/topics/pages"), name);
const topics = createRoute({ getParentRoute: () => root, path: "/topics", validateSearch: topicsSearch, component: topicsPage("TopicsPage") });
const topic = createRoute({ getParentRoute: () => root, path: "/topics/$topic", validateSearch: topicSearch, component: topicsPage("TopicLayout") });
const topicIndex = createRoute({ getParentRoute: () => topic, path: "/", validateSearch: messagesSearch, component: topicsPage("MessagesPage") });
const topicMessages = createRoute({ getParentRoute: () => topic, path: "/messages", validateSearch: messagesSearch, component: topicsPage("MessagesPage") });
const topicPartitions = createRoute({ getParentRoute: () => topic, path: "/partitions", validateSearch: partitionSearch, component: topicsPage("PartitionsPage") });
const topicLog = createRoute({ getParentRoute: () => topic, path: "/log", validateSearch: partitionSearch, component: topicsPage("LogPage") });
const topicConfig = createRoute({ getParentRoute: () => topic, path: "/config", component: topicsPage("TopicConfigPage") });
const topicConsumers = createRoute({ getParentRoute: () => topic, path: "/consumers", component: topicsPage("ConsumersPage") });
const topicProducers = createRoute({ getParentRoute: () => topic, path: "/producers", validateSearch: partitionSearch, component: topicsPage("ProducersPage") });
const topicTxn = createRoute({ getParentRoute: () => topic, path: "/transactions/$txn", component: topicsPage("TxnPage") });
const topicTxnHeld = createRoute({ getParentRoute: () => topic, path: "/transactions/$txn/held", component: topicsPage("TxnHeldPage") });
const topicTree = topic.addChildren([topicIndex, topicMessages, topicPartitions, topicLog, topicConfig, topicConsumers, topicProducers, topicTxn, topicTxnHeld]);
const group = createRoute({
  getParentRoute: () => root,
  path: "/groups/$group",
  component: function Group() {
    const { group } = group_.useParams();
    return <PlannedPage title={group} slice="R2" classicPath={`/groups/${encodeURIComponent(group)}`} crumbs={[{ label: "Groups", to: "/groups" }]} />;
  },
});
const group_ = group;

const tree = root.addChildren([
  home,
  connect,
  gallery,
  planned("/live", "Live", "R3"),
  topics,
  topicTree,
  planned("/groups", "Groups", "R2"),
  group,
  planned("/access", "Access", "R4"),
  planned("/lab", "Lab", "R4"),
  planned("/settings", "Settings", "R4"),
]);

export const router = createRouter({
  routeTree: tree,
  basepath: basePath || "/",
  defaultPreload: false,
  scrollRestoration: true,
  defaultStructuralSharing: true,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
