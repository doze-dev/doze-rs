import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { CopyIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { CommandPalette, type PaletteGroup } from "@doze/ui/components/app/command-palette";
import { copyText } from "@doze/ui/components/app/copy";
import { ThemeSwitch } from "@doze/ui/components/app/theme-menu";
import { TopBar, topBarChip } from "@doze/ui/components/app/top-bar";
import { Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail } from "@doze/ui/components/ui/sidebar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@doze/ui/components/ui/tooltip";
import { useSidebarOpen } from "@doze/ui/lib/sidebar-state";
import { setTheme } from "@doze/ui/lib/theme";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../api/queries";
import { useResourcesIndex } from "../api/resources";
import { iconOf } from "../pages/resources/model";
import { isActive, RECENTS_KEY, SPACES, type SpacePath } from "../nav";
import { iamWords } from "../words";
import { Logo, Wordmark } from "./brand";

/** The rail: the mark, and the six spaces. */
function Rail() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <Sidebar collapsible="icon" data-testid="sidebar">
      <SidebarHeader className="h-[52px] justify-center border-b border-sidebar-border px-4 group-data-[collapsible=icon]:px-2">
        <Link to="/" className="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50" aria-label="doze-aws, Home" data-testid="brand">
          <Logo size={30} className="rounded-[8px]" />
          <Wordmark className="text-[16px] leading-none group-data-[collapsible=icon]:hidden" />
        </Link>
      </SidebarHeader>
      <SidebarContent className="pt-4">
        <SidebarGroup>
          <SidebarGroupContent>
            <nav aria-label="Spaces">
              <SidebarMenu className="gap-0.5">
                {SPACES.map((space) => {
                  const active = isActive(space, path);
                  return (
                    <SidebarMenuItem key={space.to}>
                      <SidebarMenuButton asChild isActive={active} tooltip={space.label} className="h-10 gap-3 rounded-[9px] text-[14px] data-[active=true]:font-medium [&>svg]:size-[18px] [&>svg]:text-muted-foreground data-[active=true]:[&>svg]:text-primary">
                        <Link to={space.to} aria-current={active ? "page" : undefined} data-testid={`space-${space.label.toLowerCase()}`}>
                          <space.icon />
                          <span>{space.label}</span>
                        </Link>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </nav>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarRail />
    </Sidebar>
  );
}

/** The top bar's right side: where SDKs connect (a click copies it), the default region, what IAM does. Each is what
 * `/api/v1/about` says; while it is not known, or could not be had, there is no chip (Home says why). */
function Chips() {
  const about = useAbout().data;
  if (!about) return null;
  const iam = iamWords(about.iam);
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <button type="button" className={cn(topBarChip, "hidden font-mono transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none md:inline-flex")} onClick={() => void copyText(about.endpoint, "Endpoint copied")} aria-label={`Copy the endpoint ${about.endpoint}`} data-testid="chip-endpoint">
            <span>{about.endpoint.replace(/^https?:\/\//, "")}</span>
            <CopyIcon aria-hidden className="size-3.5 text-muted-foreground" />
          </button>
        </TooltipTrigger>
        <TooltipContent>The address AWS SDKs and the AWS CLI reach doze-aws at. Click to copy it.</TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className={cn(topBarChip, "hidden font-mono outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 lg:inline-flex")} data-testid="chip-region">
            {about.region}
          </span>
        </TooltipTrigger>
        <TooltipContent>The default region (--region).</TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className={cn(topBarChip, "hidden outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 lg:inline-flex")} data-testid="chip-iam">
            <span aria-hidden className={cn("size-[7px] shrink-0 rounded-full", about.iam === "off" ? "border-[1.5px] border-muted-foreground" : "bg-primary")} />
            {iam.chip}
          </span>
        </TooltipTrigger>
        <TooltipContent>{iam.sentence}</TooltipContent>
      </Tooltip>
    </>
  );
}

/** The frame of every page: the rail, the top bar with the palette's trigger, the page, and the palette itself. */
export function Shell() {
  const [palette, setPalette] = useState(false);
  const [sidebar, setSidebar] = useSidebarOpen();
  const navigate = useNavigate();
  // Where the focus goes back when the palette closes: what had it when the palette opened, else the top bar's trigger.
  const opener = useRef<HTMLElement | null>(null);
  const trigger = () => document.querySelector<HTMLElement>("[data-testid=palette-trigger]");
  const openPalette = (o: boolean) => {
    if (o) {
      const active = document.activeElement as HTMLElement | null;
      opener.current = active && active !== document.body ? active : trigger();
    }
    setPalette(o);
  };
  // Every resource by name, counted when the palette is first opened (each kind is one list call, which Live shows), and again
  // when the counts are older than the query's stale time.
  const index = useResourcesIndex(palette);
  const resources = useMemo<PaletteGroup | null>(() => {
    const items = (index.data?.services ?? []).flatMap((s) =>
      s.kinds.flatMap((k) =>
        k.names.map((name) => ({
          label: name,
          to: `/resources/${encodeURIComponent(s.name)}/${encodeURIComponent(k.id)}/${encodeURIComponent(name)}`,
          kind: k.one,
          icon: iconOf(s.name),
          mono: true,
          keywords: `${s.title} ${k.title}`,
        })),
      ),
    );
    return items.length > 0 ? { heading: "Resources", separated: true, items } : null;
  }, [index.data]);
  const groups = useMemo<PaletteGroup[]>(
    () => [
      { heading: "Spaces", items: SPACES.map((s) => ({ label: s.label, to: s.to, kind: "space", icon: s.icon, keywords: s.keywords })) },
      {
        heading: "Theme",
        separated: true,
        items: [
          { label: "Light theme", to: "theme:light", kind: "theme", icon: SunIcon, keywords: "mode appearance paper", run: () => setTheme("light") },
          { label: "Dark theme", to: "theme:dark", kind: "theme", icon: MoonIcon, keywords: "mode appearance night", run: () => setTheme("dark") },
          { label: "Theme of the system", to: "theme:system", kind: "theme", icon: MonitorIcon, keywords: "mode appearance os auto", run: () => setTheme("system") },
        ],
      },
      ...(resources ? [resources] : []),
    ],
    [resources],
  );
  return (
    // 184 px, the boards' rail. The shared Sidebar sets its width inline, so it is given as a style, not a class.
    <SidebarProvider style={{ "--sidebar-width": "11.5rem" } as React.CSSProperties} open={sidebar} onOpenChange={setSidebar}>
      <Rail />
      <SidebarInset className="min-w-0 bg-background">
        <TopBar onPalette={() => openPalette(true)} paletteLabel="Jump to a space or a resource">
          <Chips />
          <ThemeSwitch />
        </TopBar>
        <main id="main" tabIndex={-1} className="mx-auto w-full max-w-[1600px] min-w-0 px-6 pt-7 pb-20 outline-none lg:px-10 xl:px-12" data-testid="page">
          <Outlet />
        </main>
        <CommandPalette
          open={palette}
          onOpenChange={openPalette}
          returnFocus={() => (opener.current?.isConnected ? opener.current : trigger())}
          groups={groups}
          onGo={(item) => void navigate({ to: item.to as SpacePath })}
          title="Jump to a space or a resource"
          description="Type a name, then Enter"
          placeholder="Jump to a space or a resource, or switch the theme…"
          recentsKey={RECENTS_KEY}
        />
      </SidebarInset>
    </SidebarProvider>
  );
}
