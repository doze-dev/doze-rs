import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { cn } from "@doze/ui/lib/utils";
import { RehearseTab } from "./rehearse-tab";
import { type LabSearch, type LabTab, tabOf } from "./search";
import { SendTab } from "./send-tab";

const TAB_LABELS: Record<LabTab, string> = { rehearse: "Rehearse", send: "Send a call" };

/** The Lab's tabs, as links (the tab is in the address, so a reload and a link show the same one). */
function Tabs({ tab }: { tab: LabTab }) {
  return (
    <nav aria-label="Lab" className="mb-8 flex gap-8 border-b" data-testid="lab-tabs">
      {(Object.keys(TAB_LABELS) as LabTab[]).map((t) => (
        <Link key={t} to="/lab" search={t === "rehearse" ? {} : { tab: t }} aria-current={t === tab ? "page" : undefined} className={cn("-mb-px border-b-2 px-0.5 pt-3 pb-3 text-[14px] whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", t === tab ? "border-primary font-medium text-bright" : "border-transparent text-muted-foreground hover:text-foreground")} data-testid={`lab-tab-${t}`}>
          {TAB_LABELS[t]}
        </Link>
      ))}
    </nav>
  );
}

/** The Lab: rehearse what production does to a client, and send any call doze-aws has built. */
export function LabPage() {
  const search = useSearch({ from: "/lab" });
  const navigate = useNavigate({ from: "/lab" });
  const tab = tabOf(search);
  const pick = (next: LabSearch) => void navigate({ search: next });
  return (
    <>
      <PageHeader title="Lab" sub="Rehearse what production does to a client, and send any call doze-aws has built, as this console." />
      <Tabs tab={tab} />
      {tab === "rehearse" ? <RehearseTab /> : <SendTab api={search.api ?? ""} op={search.op ?? ""} pick={pick} />}
    </>
  );
}
