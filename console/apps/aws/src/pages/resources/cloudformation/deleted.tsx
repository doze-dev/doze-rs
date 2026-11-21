// A stack's address that CloudFormation no longer answers to (cloudformation__stack-deleted): when it was deleted, the record
// ListStacks keeps by the stack's ID (read by that ID: its events, its template for what it kept), or, when no such record exists,
// that there never was a stack of that name.
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Callout } from "@doze/ui/components/app/callout";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { DetailBadgeView, DetailError, RuleHelp } from "../../../kit";
import { at } from "../deep/parts";
import { useCfnRead, useEvents, useResources, useStack, useTemplate, templateText } from "./api";
import { deletedRecord, nameList, retainedIn } from "./model";
import { EventsTable, Lede, Section, ToStacks, statusBadge } from "./parts";
import { deletedSaid } from "./sentences";


export function DeletedStack({ name }: { name: string }) {
  const list = useCfnRead<{ StackSummaries?: { StackName?: string; StackId?: string; DeletionTime?: number | string }[] }>("ListStacks", {});
  const record = list.data ? deletedRecord(list.data.StackSummaries ?? [], name) : null;
  const id = record?.id ?? "";
  const stackQ = useStack(id, Boolean(record));
  const events = useEvents(id, undefined, Boolean(record));
  const resources = useResources(id, Boolean(record));
  const template = useTemplate(id, "Original", Boolean(record));
  const stack = stackQ.data?.Stacks?.[0];
  if (list.isPending) return <Skeleton className="h-28 rounded-[14px]" data-testid="detail-loading" />;
  if (list.isError) return <DetailError service="cloudformation" kind="stacks" name={name} what="The deleted stacks could not be read" error={list.error} header={false} testId="detail-error" />;
  if (!record) return <DetailError service="cloudformation" kind="stacks" name={name} error={new ApiFailure(404, { code: "ValidationError", message: `Stack with id ${name} does not exist` })} testId="stack-none" />;
  const kept = retainedIn(templateText(template.data?.TemplateBody)).map((r) => r.id);
  const left = resources.data?.StackResourceSummaries ?? [];
  const said = deletedSaid({ name, status: "DELETE_COMPLETE", created: null, updated: null, resources: left, exports: [], importers: {}, failed: null, retained: kept });
  const crumbs = [{ label: "Resources", to: "/resources" }, { label: "Infrastructure", to: "/resources" }, { label: "CloudFormation", to: "/resources/cloudformation" }, { label: `${name}, deleted`, to: "/resources/cloudformation/stacks" }];
  return (
    <div data-testid="stack-gone">
      <PageHeader title={name} mono crumbs={crumbs} titleExtra={<DetailBadgeView badge={{ ...statusBadge("DELETE_COMPLETE"), tone: "neutral" }} />} sub={`${stack?.CreationTime ? `Created ${at(stack.CreationTime)}. ` : ""}Deleted ${at(stack?.DeletionTime ?? record.deleted)}.`} actions={<ToStacks />} />
      <Lede said={said} testId="stack-deleted-sentence" />
      <Callout tone="info" lead="A deleted stack is hidden from the list by default." className="mb-8" testId="stack-deleted">Choose Deleted in Filter status to see it. CloudFormation keeps the record by its stack ID, <span className="font-mono text-[12.5px] break-all">{record.id}</span>; by name it is gone.</Callout>
      <div className="grid gap-8">
        <Section title="Resources" lede={left.length === 0 ? "ListStackResources returns nothing for this deleted stack." : "What ListStackResources returns for the deleted stack, read by its ID."} testId="gone-resources">
          <RuleHelp className="max-w-[56rem]">
            {left.length > 0 ? `ListStackResources still answers by the stack ID: ${left.length} ${left.length === 1 ? "resource" : "resources"}, ${left.every((r) => r.ResourceStatus === "DELETE_COMPLETE") ? "each DELETE_COMPLETE" : "with their last statuses"}. ` : "The stack record keeps its events and gives up its resource list: what it deleted is gone. "}
            {kept.length > 0 ? <>{nameList(kept)} {kept.length === 1 ? "is" : "are"} no longer managed by CloudFormation: its template said DeletionPolicy Retain, so it was left in place. Delete {kept.length === 1 ? "it" : "them"} yourself when you no longer want {kept.length === 1 ? "it" : "them"}.</> : "Nothing was kept: no resource of its template had DeletionPolicy Retain."}
          </RuleHelp>
        </Section>
        <Section title="Events" lede="Newest first. The delete adds its events on top of the stack's creation." testId="gone-events">
          {events.data ? <EventsTable events={events.data.StackEvents ?? []} /> : <Skeleton className="h-40 rounded-[12px]" />}
        </Section>
      </div>
    </div>
  );
}
