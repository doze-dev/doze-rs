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
  return (
    <>
      <PageHeader title="Connect your app" crumbs={[{ label: "Home", to: "/" }, { label: "Connect your app", to: "/connect" }]} sub="Point an AWS SDK or the AWS CLI at this doze-aws. Running on its own it has one address, and that one address serves every service." />
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
