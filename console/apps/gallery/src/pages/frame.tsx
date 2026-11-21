import { ChevronsUpDownIcon, PlusIcon } from "lucide-react";
import { useCallback, useState } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ThemeSwitch } from "@doze/ui/components/app/theme-menu";
import { topBarChip } from "@doze/ui/components/app/top-bar";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@doze/ui/components/ui/breadcrumb";
import { Button } from "@doze/ui/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@doze/ui/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@doze/ui/components/ui/collapsible";
import { ScrollArea } from "@doze/ui/components/ui/scroll-area";
import { Separator } from "@doze/ui/components/ui/separator";
import { SidebarMenu, SidebarMenuItem, SidebarMenuSkeleton } from "@doze/ui/components/ui/sidebar";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { Section, Specimen } from "../specimen";

const AT = Date.UTC(2026, 10, 19, 14, 32, 7);

export function FramePage() {
  const [updatedAt, setUpdatedAt] = useState(AT);
  const [fetching, setFetching] = useState(false);
  const refresh = useCallback(() => {
    setFetching(true);
    window.setTimeout(() => {
      setUpdatedAt((t) => t + 5_000);
      setFetching(false);
    }, 600);
  }, []);
  const [tab, setTab] = useState("messages");
  return (
    <>
      <PageHeader title="Frame" sub="The pieces every page stands in. The gallery's own sidebar, top bar and palette are the shared ones: this page shows the rest." />

      <Section id="page-header" title="PageHeader" source="components/app/page-header.tsx" note="The ancestors only as a breadcrumb, the page's name once, a static sentence, the actions, and the refresh control.">
        <Specimen label="With ancestors" stack>
          <div className="w-full rounded-xl border bg-background px-6 pt-6">
            <PageHeader
              title="orders"
              mono
              crumbs={[
                { label: "Resources", to: "/data" },
                { label: "Queues", to: "/data" },
              ]}
              sub="A standard queue. Dead letters go to pick-lists-dlq after 5 receives."
              refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} />}
              actions={
                <>
                  <Button variant="outline">Peek</Button>
                  <Button>
                    <PlusIcon /> Send a message
                  </Button>
                </>
              }
            />
          </div>
        </Specimen>
        <Specimen label="A top page" stack>
          <div className="w-full rounded-xl border bg-background px-6 pt-6">
            <PageHeader title="Twelve queues" sub="Every queue of this stack, with what is waiting in it." actions={<Button>Create queue</Button>} />
          </div>
        </Specimen>
      </Section>

      <Section id="refresh" title="RefreshControl" source="components/app/refresh-control.tsx · lib/refresh.ts" note="The time of the data on screen, one interval for every page (remembered), and Refresh, which is also the r key.">
        <Specimen label="With an interval">
          <RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} />
        </Specimen>
        <Specimen label="Not loaded, no interval">
          <RefreshControl updatedAt={0} fetching={false} onRefresh={refresh} interval={false} />
        </Specimen>
      </Section>

      <Section id="top-bar" title="TopBar pieces" source="components/app/top-bar.tsx · theme-menu.tsx">
        <Specimen label="Theme">
          <ThemeSwitch />
        </Specimen>
        <Specimen label="Chips">
          <span className={`${topBarChip} font-mono`}>
            <span className="size-[7px] rounded-full bg-primary" aria-hidden />
            127.0.0.1:4566
          </span>
          <span className={topBarChip}>
            <span className="size-[7px] rounded-full bg-good" aria-hidden />
            IAM enforced
          </span>
          <span className={`${topBarChip} border-warn/45 text-warn`}>Unprotected</span>
        </Specimen>
      </Section>

      <Section id="tabs" title="Tabs and Breadcrumb" source="components/ui/tabs.tsx · breadcrumb.tsx">
        <Specimen label="Tabs" stack>
          <Tabs value={tab} onValueChange={setTab} className="w-full max-w-[34rem]" data-testid="tabs">
            <TabsList>
              <TabsTrigger value="messages">Messages</TabsTrigger>
              <TabsTrigger value="config">Configuration</TabsTrigger>
              <TabsTrigger value="access">Access</TabsTrigger>
              <TabsTrigger value="off" disabled>
                Metrics
              </TabsTrigger>
            </TabsList>
            <TabsContent value="messages" className="rounded-xl border bg-card p-4 text-[13.5px]">
              3 waiting, 0 in flight.
            </TabsContent>
            <TabsContent value="config" className="rounded-xl border bg-card p-4 text-[13.5px]">
              Hidden for 30 s on receive.
            </TabsContent>
            <TabsContent value="access" className="rounded-xl border bg-card p-4 text-[13.5px]">
              Two roles may send.
            </TabsContent>
          </Tabs>
        </Specimen>
        <Specimen label="Line tabs">
          <Tabs defaultValue="events">
            <TabsList variant="line">
              <TabsTrigger value="events">Events</TabsTrigger>
              <TabsTrigger value="resources">Resources</TabsTrigger>
              <TabsTrigger value="outputs">Outputs</TabsTrigger>
            </TabsList>
          </Tabs>
        </Specimen>
        <Specimen label="Breadcrumb">
          <Breadcrumb>
            <BreadcrumbList>
              <BreadcrumbItem>
                <BreadcrumbLink href="#">Resources</BreadcrumbLink>
              </BreadcrumbItem>
              <BreadcrumbSeparator />
              <BreadcrumbItem>
                <BreadcrumbLink href="#">Queues</BreadcrumbLink>
              </BreadcrumbItem>
              <BreadcrumbSeparator />
              <BreadcrumbItem>
                <BreadcrumbPage>orders</BreadcrumbPage>
              </BreadcrumbItem>
            </BreadcrumbList>
          </Breadcrumb>
        </Specimen>
      </Section>

      <Section id="card" title="Card, Collapsible, ScrollArea and Separator" source="components/ui/card.tsx · collapsible.tsx · scroll-area.tsx · separator.tsx">
        <Specimen label="Card" className="items-stretch">
          <Card className="w-80">
            <CardHeader>
              <CardTitle>Connect</CardTitle>
              <CardDescription>Point an SDK at this endpoint.</CardDescription>
            </CardHeader>
            <CardContent>
              <code className="text-[13px] text-bright">http://127.0.0.1:4566</code>
            </CardContent>
            <CardFooter>
              <Button variant="outline" size="sm">
                Show the snippet
              </Button>
            </CardFooter>
          </Card>
          <Card className="w-80">
            <CardHeader>
              <CardTitle>A card with a long title that wraps onto a second line</CardTitle>
              <CardDescription>And a description that is long enough to wrap as well, so that the rhythm of the two is seen.</CardDescription>
            </CardHeader>
          </Card>
        </Specimen>
        <Specimen label="Collapsible">
          <Collapsible className="w-80 rounded-xl border bg-card px-4 py-2">
            <div className="flex items-center justify-between">
              <span className="text-[13.5px] font-medium text-bright">Advanced</span>
              <CollapsibleTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label="Show the advanced settings" data-testid="collapsible-trigger">
                  <ChevronsUpDownIcon />
                </Button>
              </CollapsibleTrigger>
            </div>
            <CollapsibleContent className="pt-1 pb-2 text-[13.5px] text-muted-foreground" data-testid="collapsible-content">
              Delivery delay, receive wait time, redrive policy.
            </CollapsibleContent>
          </Collapsible>
        </Specimen>
        <Specimen label="ScrollArea">
          <ScrollArea className="h-36 w-64 rounded-xl border bg-card">
            <ul className="p-3 text-[13px]">
              {Array.from({ length: 24 }, (_, i) => (
                <li key={i} className="py-1 font-mono">
                  orders-{String(i).padStart(4, "0")}
                </li>
              ))}
            </ul>
          </ScrollArea>
        </Specimen>
        <Specimen label="Separator">
          <div className="flex h-6 items-center gap-3 text-[13.5px]">
            <span>Waiting</span>
            <Separator orientation="vertical" />
            <span>In flight</span>
            <Separator orientation="vertical" />
            <span>Delayed</span>
          </div>
        </Specimen>
      </Section>

      <Section id="sidebar" title="Sidebar" source="components/ui/sidebar.tsx" note="The gallery's rail is the shared Sidebar: icons only below 1280 px or by its trigger (also ⌘B / Ctrl B), remembered per browser.">
        <Specimen label="While its counts load">
          <SidebarMenu className="w-56 rounded-xl border bg-sidebar p-2" aria-busy aria-label="Loading navigation">
            {[0, 1, 2].map((i) => (
              <SidebarMenuItem key={i}>
                <SidebarMenuSkeleton showIcon />
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </Specimen>
      </Section>
    </>
  );
}
