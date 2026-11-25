import { useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useAbout } from "../../api/queries";
import { useArrivals } from "../../api/spaces";
import { CardSkeleton } from "../card";
import { Code } from "./connect-code";
import { Overview } from "./connect-overview";
import { ByService } from "./connect-services";

type Tab = "overview" | "code" | "services";
const TABS: readonly Tab[] = ["overview", "code", "services"];

/** What the page says of itself on each tab: the board's subtitle (one address here, so no `.doze name`), and the crumb that names the tab. */
export const TAB_WORDS: Record<Tab, { sub: string; crumb: string | null }> = {
  overview: { sub: "Point an AWS SDK or the AWS CLI at this doze-aws. Running on its own it has one address, and that one address serves every service.", crumb: null },
  code: { sub: "The same call, list the queues, from each client.", crumb: "Code" },
  services: { sub: "Where each service answers, on IP and port.", crumb: "By service" },
};

/** Connect your app: the address and the environment (Overview), the same call from each client (Code), and where each service
 * answers (By service). Everything is from `/api/v1/about`, `/api/v1/services` and the Live record. */
export function ConnectPage() {
  const about = useAbout();
  const qc = useQueryClient();
  // The tab is in the address (`?tab=code`), so a reload, a pasted link and the sheets of By service keep their place.
  const [asked, setAsked] = useUrlParam("tab");
  const tab: Tab = TABS.find((t) => t === asked) ?? "overview";
  const setTab = useCallback((t: Tab) => setAsked(t === "overview" ? undefined : t), [setAsked]);
  const onCalls = useCallback(() => void qc.invalidateQueries({ queryKey: ["home"], exact: true }), [qc]);
  useArrivals(onCalls);
  const a = about.data;
  const words = TAB_WORDS[tab];
  return (
    <>
      <PageHeader
        title="Connect your app"
        crumbs={[{ label: "Home", to: "/" }, { label: "Connect your app", to: words.crumb ? "/connect" : "" }, ...(words.crumb ? [{ label: words.crumb, to: "" }] : [])]}
        sub={words.sub}
      />
      {!a ? (
        <CardSkeleton rows={4} />
      ) : (
        <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
          <TabsList className="mb-8" data-testid="connect-tabs">
            <TabsTrigger value="overview" data-testid="tab-overview">
              Overview
            </TabsTrigger>
            <TabsTrigger value="code" data-testid="tab-code">
              Code
            </TabsTrigger>
            <TabsTrigger value="services" data-testid="tab-services">
              By service
            </TabsTrigger>
          </TabsList>
          <TabsContent value="overview">
            <Overview a={a} goTo={setTab} />
          </TabsContent>
          <TabsContent value="code">
            <Code a={a} goTo={setTab} />
          </TabsContent>
          <TabsContent value="services">
            <ByService a={a} />
          </TabsContent>
        </Tabs>
      )}
    </>
  );
}
