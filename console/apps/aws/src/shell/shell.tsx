import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { CopyIcon, Link2Icon, MonitorIcon, MoonIcon, SendIcon, ShieldAlertIcon, SunIcon } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { CommandPalette, type PaletteGroup, type PaletteItem } from "@doze/ui/components/app/command-palette";
import { copyText } from "@doze/ui/components/app/copy";
import { ThemeSwitch } from "@doze/ui/components/app/theme-menu";
import { TopBar, topBarChip } from "@doze/ui/components/app/top-bar";
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail } from "@doze/ui/components/ui/sidebar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@doze/ui/components/ui/tooltip";
import { number } from "@doze/ui/lib/format";
import { useSidebarOpen } from "@doze/ui/lib/sidebar-state";
import { setTheme } from "@doze/ui/lib/theme";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../api/queries";
import { useResourcesIndex } from "../api/resources";
import { subscriptionLabel } from "../pages/resources/sns/sub-model";
import { iconOf } from "../pages/resources/model";
import { isActive, RECENTS_KEY, SPACES, type SpacePath } from "../nav";
import { iamWords } from "../words";
import { Logo, Wordmark } from "./brand";
import { IdentityChip } from "./identity-chip";
import { PinButton, RailPlaces } from "./places";
import { RailServices, useRailCounts } from "./rail-services";

/** The rail: the mark, and the six spaces. */
function Rail() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const counts = useRailCounts();
  const countOf = (to: string): { text: string; title: string } | null => {
    if (to === "/live" && counts.live !== undefined) return { text: number(counts.live), title: `${number(counts.live)} ${counts.live === 1 ? "app has" : "apps have"} called` };
    if (to === "/resources" && counts.awake) return { text: number(counts.awake.awake), title: `${counts.awake.awake} of ${counts.awake.served} services awake` };
    return null;
  };
  return (
    <Sidebar collapsible="icon" data-testid="sidebar">
      <SidebarHeader className="h-[52px] justify-center border-b border-sidebar-border px-4 group-data-[collapsible=icon]:px-2">
        <Link to="/" className="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50" aria-label="doze-aws, Home" data-testid="brand">
          <Logo size={32} className="rounded-[8px]" />
          <Wordmark className="text-[20px] leading-none group-data-[collapsible=icon]:hidden" />
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
                      <SidebarMenuButton asChild isActive={active} tooltip={space.label} className="h-9 text-[14px] data-[active=true]:font-medium [&>svg]:text-muted-foreground data-[active=true]:[&>svg]:text-primary">
                        <Link to={space.to} aria-current={active ? "page" : undefined} data-testid={`space-${space.label.toLowerCase()}`}>
                          <space.icon />
                          <span>{space.label}</span>
                        </Link>
                      </SidebarMenuButton>
                      {countOf(space.to) && (
                        <SidebarMenuBadge className="font-normal text-muted-foreground tabular-nums" title={countOf(space.to)?.title} data-testid={`count-${space.label.toLowerCase()}`}>
                          {countOf(space.to)?.text}
                        </SidebarMenuBadge>
                      )}
                      {space.to === "/resources" && active && <RailServices />}
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </nav>
          </SidebarGroupContent>
        </SidebarGroup>
        <RailPlaces />
      </SidebarContent>
      <SidebarFooter className="px-4 pb-4 group-data-[collapsible=icon]:hidden" data-testid="rail-footer">
        <p className="flex items-center gap-2 px-1 text-[12px] text-muted-foreground">
          <kbd className="rounded border bg-card px-1.5 py-px font-mono text-[11px]">⌘K</kbd> jump or do
        </p>
      </SidebarFooter>
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
      <IdentityChip />
      {/* What this doze-aws is set to, in one pill: its region and what IAM does (each with its own tooltip). */}
      <span className={cn(topBarChip, "hidden gap-0 divide-x px-0 lg:inline-flex")} data-testid="chip-settings">
        <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={0} className="inline-flex h-full items-center rounded-l-full px-3 font-mono outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50" data-testid="chip-region">
              {about.region}
            </span>
          </TooltipTrigger>
          <TooltipContent>The default region (--region).</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={0} className="inline-flex h-full items-center gap-2 rounded-r-full px-3 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50" data-testid="chip-iam">
              <span aria-hidden className={cn("size-[7px] shrink-0 rounded-full", about.iam === "off" ? "border-[1.5px] border-muted-foreground" : "bg-primary")} />
              {iam.chip}
            </span>
          </TooltipTrigger>
          <TooltipContent>{iam.sentence}</TooltipContent>
        </Tooltip>
      </span>
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
  // A service that is asleep is not listed to be searched (listing it would wake it) until the person asks, from the palette.
  const [wake, setWake] = useState(false);
  const index = useResourcesIndex(palette, wake);
  const sleeping = (index.data?.services ?? []).some((s) => s.asleep);
  const resources = useMemo<PaletteGroup | null>(() => {
    const items = (index.data?.services ?? []).flatMap((s) =>
      s.kinds.flatMap((k) =>
        k.names.map((name) => ({
          label: s.name === "sns" && k.id === "subscriptions" ? subscriptionLabel(name) : name,
          to: `/resources/${encodeURIComponent(s.name)}/${encodeURIComponent(k.id)}/${encodeURIComponent(name)}`,
          kind: k.one,
          icon: iconOf(s.name),
          mono: true,
          keywords: `${s.title} ${k.title} ${name}`,
        })),
      ),
    );
    return items.length > 0 ? { heading: "Resources", separated: true, items } : null;
  }, [index.data]);
  const about = useAbout().data;
  const actions = useMemo<PaletteItem[]>(() => {
    const queues = (index.data?.services ?? []).filter((s) => s.name === "sqs").flatMap((s) => s.kinds.filter((k) => k.id === "queues").flatMap((k) => k.names)).slice(0, 20);
    return [
      ...(about ? [{ label: "Copy the endpoint", to: "do:endpoint", kind: "action", icon: CopyIcon, keywords: "address url sdk aws_endpoint_url", run: () => void copyText(about.endpoint, "Endpoint copied") }] : []),
      ...(sleeping ? [{ label: "Search the sleeping services too", to: "do:wake", kind: "action", icon: MoonIcon, keywords: "asleep wake resources names count find", run: () => setWake(true) }] : []),
      { label: "Connect your app", to: "do:connect", kind: "action", icon: Link2Icon, keywords: "snippets environment sdk cli", run: () => void navigate({ to: "/connect" }) },
      { label: "Explain the last refusal", to: "do:refusal", kind: "action", icon: ShieldAlertIcon, keywords: "denied would be denied iam access policy", run: () => void navigate({ to: "/access" }) },
      ...queues.map((name) => ({ label: `Send a message to ${name}`, to: `do:send:${name}`, kind: "action", icon: SendIcon, mono: false, keywords: `queue sqs peek ${name}`, run: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "sqs", kind: "queues", name }, search: { view: "messages" } }) })),
    ];
  }, [about, index.data, navigate, sleeping]);
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
      // After the names: typing a resource's name and pressing Enter opens it; an action is chosen by its words.
      ...(actions.length > 0 ? [{ heading: "Do", separated: true, items: actions }] : []),
    ],
    [resources, actions],
  );
  return (
    // 224 px: the boards drew 184 and the Kafka console's rail is 256; the services under Resources and the counts need the room, and a wide table needs the rest. The shared Sidebar sets its width inline, so it is given as a style, not a class.
    // bg-sidebar on the wrapper: the rail is fixed to the window, so a page taller than the window (a full-page capture) shows the
    // wrapper beside the inset below it; in the rail's colour, the rail reads as reaching the bottom.
    <SidebarProvider className="bg-sidebar" style={{ "--sidebar-width": "14rem" } as React.CSSProperties} open={sidebar} onOpenChange={setSidebar}>
      <Rail />
      <SidebarInset className="min-w-0 bg-background">
        <TopBar onPalette={() => openPalette(true)} paletteLabel="Jump to a space or a resource" afterPalette={<PinButton />}>
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
