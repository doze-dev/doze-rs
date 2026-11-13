import { createRootRoute, createRoute, createRouter, lazyRouteComponent, Outlet } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { basePath } from "@/api/client";
import { AppSidebar } from "@/components/app/app-sidebar";
import { CommandPalette } from "@/components/app/command-palette";
import { TopBar } from "@/components/app/top-bar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { ConnectPage } from "@/pages/connect";
import { configSearch, connectSearch, createSearch, gallerySearch, topicWriteSearch } from "@/lib/search";
import { HomePage } from "@/pages/home";
import { NotFoundPage, PlannedPage } from "@/pages/planned";
import { TopicFrameStub } from "@/pages/topics/frame-stub";

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

// Slice R1b: Create topic, and a topic's frame (a stand-in for R1a's) with the write overlays and the Config tab.
const topicsNew = createRoute({ getParentRoute: () => root, path: "/topics/new", validateSearch: createSearch, component: lazyRouteComponent(() => import("@/pages/topics/create"), "CreateTopicPage") });
const topic = createRoute({ getParentRoute: () => root, path: "/topics/$topic", validateSearch: topicWriteSearch, component: TopicFrameStub });
const topicTab = <const P extends string>(path: P, title: string) =>
  createRoute({
    getParentRoute: () => topic,
    path,
    component: function TopicTab() {
      const { topic: t } = topic.useParams();
      return <p className="text-[14px] text-muted-foreground">{title} is being rebuilt (slice R1a). <a className="text-primary hover:underline" href={`${basePath}/classic/topics/${encodeURIComponent(t)}${path === "/" ? "" : `/${path}`}`}>Open it in the classic console</a>.</p>;
    },
  });
const topicConfig = createRoute({
  getParentRoute: () => topic,
  path: "config",
  validateSearch: configSearch,
  component: lazyRouteComponent(() => import("@/pages/topics/config-route"), "ConfigRoute"),
});
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
  planned("/topics", "Topics", "R1"),
  topicsNew,
  topic.addChildren([topicTab("/", "Messages"), topicTab("log", "Log"), topicConfig, topicTab("consumers", "Consumers"), topicTab("producers", "Producers")]),
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
