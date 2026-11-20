// A resource's deep page: the generic detail as its first tab ("Details"), then the tabs a descriptor's fields cannot be (a
// function's Test, a stack's Events…). The tab is kept in the address (`?tab=`), so a tab can be linked to and comes back
// with the page. Loaded only when a resource with a deep page is opened.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import type { ResourceDetail } from "../../api/types/ResourceDetail";
import { StackSentence, StackChangeSets, StackEvents, StackTemplate } from "./cfn/stack-tabs";
import { FunctionConfig } from "./lambda/config-tab";
import { FunctionInvocations, FunctionVersions } from "./lambda/lists";
import { FunctionTest } from "./lambda/test-tab";
import { GroupSearch } from "./logs/search-tab";
import { GroupStreams } from "./logs/streams-tab";
import { GroupTail } from "./logs/tail-tab";
import { MachineExecutions } from "./sfn/executions-tab";

type Tab = { id: string; title: string; body: (d: ResourceDetail) => React.ReactNode };

/** Each deep page's tabs after Details, by the id the server names it with (crates/aws-console/src/deep.rs). */
const PAGES: Record<string, Tab[]> = {
  "lambda-function": [
    { id: "test", title: "Test", body: (d) => <FunctionTest name={d.name} /> },
    { id: "configuration", title: "Configuration", body: (d) => <FunctionConfig name={d.name} /> },
    { id: "versions", title: "Versions and aliases", body: (d) => <FunctionVersions name={d.name} /> },
    { id: "invocations", title: "Invocations", body: (d) => <FunctionInvocations d={d} /> },
  ],
  "state-machine": [{ id: "executions", title: "Executions", body: (d) => <MachineExecutions arn={d.id} name={d.name} /> }],
  "log-group": [
    { id: "streams", title: "Log streams", body: (d) => <GroupStreams group={d.name} /> },
    { id: "search", title: "Search", body: (d) => <GroupSearch group={d.name} /> },
    { id: "tail", title: "Live Tail", body: (d) => <GroupTail group={d.name} /> },
  ],
  stack: [
    { id: "events", title: "Events", body: (d) => <StackEvents name={d.name} /> },
    { id: "template", title: "Template", body: (d) => <StackTemplate name={d.name} /> },
    { id: "changesets", title: "Change sets", body: (d) => <StackChangeSets name={d.name} /> },
  ],
};

/** What the Details tab shows first on some pages (a stack's status sentence). */
const LEADS: Record<string, (d: ResourceDetail) => React.ReactNode> = {
  stack: (d) => <StackSentence d={d} />,
};

export function DeepTabs({ d, details }: { d: ResourceDetail; details: React.ReactNode }) {
  const tabs = PAGES[d.deep ?? ""] ?? [];
  const search = useSearch({ strict: false }) as { tab?: string };
  const navigate = useNavigate();
  const tab = tabs.some((t) => t.id === search.tab) ? (search.tab as string) : "details";
  const lead = LEADS[d.deep ?? ""];
  return (
    <Tabs value={tab} onValueChange={(t) => void navigate({ to: ".", search: (s: Record<string, unknown>) => ({ ...s, tab: t === "details" ? undefined : t }), replace: true })} className="gap-0" data-testid="deep-tabs">
      <TabsList variant="line" className="mb-8 h-auto w-full justify-start gap-6 border-b p-0" aria-label="Views of this resource">
        {[{ id: "details", title: "Details" }, ...tabs].map((t) => (
          <TabsTrigger key={t.id} value={t.id} className="flex-none px-0.5 pt-2 pb-3 text-[14px]" data-testid={`tab-${t.id}`}>
            {t.title}
          </TabsTrigger>
        ))}
      </TabsList>
      <TabsContent value="details">
        {lead?.(d)}
        {details}
      </TabsContent>
      {tabs.map((t) => (
        <TabsContent key={t.id} value={t.id} data-testid={`deep-${t.id}`}>
          {tab === t.id && t.body(d)}
        </TabsContent>
      ))}
    </Tabs>
  );
}
