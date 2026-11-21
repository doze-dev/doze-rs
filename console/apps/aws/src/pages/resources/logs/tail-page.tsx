// Live Tail of a log group as a page (an extra of the console's: the boards have no Live Tail), reached from the group's Actions.
import { PageHeader } from "@doze/ui/components/app/page-header";
import { useServiceCrumbs } from "../../../kit";
import { GroupTail } from "./tail-tab";

export function TailPage({ group }: { group: string }) {
  const base = useServiceCrumbs("logs");
  return (
    <>
      <PageHeader
        title="Live Tail"
        crumbs={[...base, { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` }]}
        sub={<>Events as <span className="font-mono text-foreground">{group}</span> takes them.</>}
      />
      <GroupTail group={group} />
    </>
  );
}
