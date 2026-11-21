// The states of a stack that its page reads differently from the tabs: waiting for review (stack-review-pending), rolled back
// after a failed create (stack-rolled-back) and just updated (stack-updated): the sentence, what to do, and what the page needs
// to say it, on one page, with the events where the explanation is.
import { Callout } from "@doze/ui/components/app/callout";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { RecentCalls, RuleHelp } from "../../../kit";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { at } from "../deep/parts";
import { useCfnRead, useEvents } from "./api";
import { type ChangeSet, ChangeSetFacts, ChangesTable, ExecutesCard, ParametersCard, changeRows } from "./changeset-parts";
import { type Stack, failedResource } from "./model";
import { Lede } from "./parts";
import { stackSaid, updatedSaid } from "./sentences";
import { EventsTab } from "./tab-events";

export type StateView = "review" | "rolled" | "updated";

export function StackStates({ view, stack, d, set }: { view: StateView; stack: Stack; d: ResourceDetail; set?: string }) {
  if (view === "review") return <Review stack={stack} set={set} />;
  if (view === "rolled") return <Rolled stack={stack} d={d} />;
  return <Updated stack={stack} />;
}

function Review({ stack, set }: { stack: Stack; set?: string }) {
  const name = stack.StackName;
  const q = useCfnRead<ChangeSet>("DescribeChangeSet", { StackName: name, ChangeSetName: set ?? "" }, { enabled: Boolean(set) });
  if (!set) return <Callout tone="info" lead="No change set is left." testId="review-none">{name} was made from a change set that has since been deleted, so there is nothing to execute. Delete the stack, or make it again.</Callout>;
  if (!q.data) return <Skeleton className="h-64 rounded-[14px]" />;
  const rows = changeRows(q.data);
  const said = stackSaid({ name, status: stack.StackStatus, created: null, updated: null, resources: [], exports: [], importers: {}, failed: null, waiting: { name: set, adds: rows.map((r) => ({ logical: r.LogicalResourceId, type: r.ResourceType })) } });
  return (
    <div className="grid gap-8" data-testid="review-state">
      <Lede said={said} className="mb-0" />
      <Callout tone="info" lead="Nothing is deployed yet." testId="review-note">REVIEW_IN_PROGRESS is the status CloudFormation gives a stack that was made from a change set and has not run it. The stack exists from CreateChangeSet on, so its name is taken and a deploy tool can poll its events between that call and ExecuteChangeSet.</Callout>
      <ChangeSetFacts set={q.data} title="Change set" />
      <ChangesTable set={q.data} lede="What the template would create. Every row is an Add, because the stack is new." />
      <div className="grid items-start gap-6 lg:grid-cols-2"><ParametersCard set={q.data} /><ExecutesCard creating rows={rows} /></div>
    </div>
  );
}

function Rolled({ stack, d }: { stack: Stack; d: ResourceDetail }) {
  const name = stack.StackName;
  const events = useEvents(name);
  const list = events.data?.StackEvents ?? [];
  const failed = failedResource(list);
  const said = stackSaid({ name, status: stack.StackStatus, created: null, updated: null, resources: [], exports: [], importers: {}, failed });
  return (
    <div className="grid gap-8" data-testid="rolled-state">
      <Lede said={said} className="mb-0" />
      <Callout tone="bad" lead="Only a delete can be performed on this stack." testId="rolled-note">A stack in ROLLBACK_COMPLETE has had everything it created removed. Delete it, then create it again{failed ? <> after fixing <span className="font-mono text-[13px]">{failed.resource}</span></> : ""}.</Callout>
      <EventsTab name={name} note={false} lede={`Newest first. ${list.length} ${list.length === 1 ? "event" : "events"}, written as the create ran and was undone.`} />
      <RuleHelp className="max-w-[56rem]">The reason on the CREATE_FAILED row is the one to read: it says which resource failed and why. Rolling back is the default; with Preserve successfully provisioned resources chosen at creation the stack would sit in CREATE_FAILED instead and keep what it made.</RuleHelp>
      <RecentCalls calls={d.calls} noun="stack" scope={name} />
    </div>
  );
}

function Updated({ stack }: { stack: Stack }) {
  const name = stack.StackName;
  const events = useEvents(name);
  const said = updatedSaid(name, events.data?.StackEvents ?? []);
  return (
    <div className="grid gap-8" data-testid="updated-state">
      <Lede said={said} className="mb-0" />
      <Callout tone="info" lead="Reached from Update stack or from Execute change set." testId="updated-note">Both open the stack on its events. If a resource had failed, the stack would show UPDATE_ROLLBACK_COMPLETE here instead, with the previous template restored and what the update made deleted again.</Callout>
      <EventsTab name={name} note={false} lede="Newest first, with the update on top of the stack's creation." />
      <RuleHelp className="max-w-[56rem]">Every resource of an update gets UPDATE_IN_PROGRESS and then UPDATE_COMPLETE, the ones the update left alone with the reason no change; a resource that is new gets the same pair. An update that changes nothing is refused rather than recorded, and a change set with no changes fails the same way. Resources the new template drops are deleted by the update. The events from the stack's creation sit below these{stack.LastUpdatedTime ? `; the update ended ${at(stack.LastUpdatedTime)}` : ""}.</RuleHelp>
    </div>
  );
}
