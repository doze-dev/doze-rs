// A change set's page (cloudformation__changeset-detail): what it previews, with Execute change set and Delete change set; at
// `/resources/cloudformation/stacks/<stack>?do=changeset&name=<change set>`.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { DetailBadgeView, DetailError, PlainConfirm, withCurrent } from "../../../kit";
import { HINT } from "../index-page";
import { at } from "../deep/parts";
import type { OwnProps } from "../deep/registry";
import { useCfnRead, useCfnWrite, useStack } from "./api";
import { type ChangeSet, ChangeSetFacts, ChangesTable, ExecutesCard, ParametersCard, changeRows } from "./changeset-parts";
import { StackTo, statusBadge } from "./parts";

export function ChangeSetPage({ service, kind, name, search }: OwnProps) {
  const stackName = name ?? "";
  const setName = search.name ?? "";
  const navigate = useNavigate();
  const [removing, setRemoving] = useState(false);
  // Once it is being deleted it is not read again: it would be refused (it is gone), and the page is on its way to the stack's.
  const q = useCfnRead<ChangeSet>("DescribeChangeSet", { StackName: stackName, ChangeSetName: setName }, { enabled: !removing });
  const stack = useStack(stackName).data?.Stacks?.[0];
  const execute = useCfnWrite<unknown>("ExecuteChangeSet");
  const remove = useCfnWrite<unknown, { StackName: string; ChangeSetName: string }>("DeleteChangeSet", (i) => [i.ChangeSetName]);
  const [asking, setAsking] = useState(false);
  const back = { to: `/resources/${service}/${kind}/${encodeURIComponent(stackName)}`, label: `Back to ${stackName}` };
  const toStack = (q2: Record<string, string>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: stackName }, search: q2 });
  if (q.isError) return <DetailError service={service} kind={kind} name={setName} noun="change set" back={back} what="This change set could not be read" error={q.error} onRetry={() => void q.refetch()} retrying={q.isFetching} />;
  const set = q.data;
  if (!set) return <div className="grid gap-4"><Skeleton className="h-12 w-96" /><Skeleton className="h-64 rounded-[var(--radius-card)]" /></div>;
  const rows = changeRows(set);
  const creating = stack?.StackStatus === "REVIEW_IN_PROGRESS";
  const ready = set.ExecutionStatus === "AVAILABLE";
  const run = () => execute.mutate({ StackName: stackName, ChangeSetName: setName }, { onSuccess: () => toStack({ tab: "events", done: creating ? "executed" : "updated" }) });
  const crumbs = [{ label: "Resources", to: "/resources" }, { label: "CloudFormation", to: `/resources/${service}` }, { label: stackName, to: back.to }, { label: "Change sets", to: `${back.to}?tab=changesets` }, { label: setName, to: "" }];
  return (
    <div data-testid="changeset-page">
      <PageHeader
        title={set.ChangeSetName}
        mono
        crumbs={withCurrent(crumbs, set.ChangeSetName)}
        titleExtra={<span className="flex flex-wrap items-center gap-2"><DetailBadgeView badge={statusBadge(set.Status)} />{set.ExecutionStatus && <DetailBadgeView badge={{ text: set.ExecutionStatus, tone: "neutral" }} />}</span>}
        sub={<>A change set for <StackTo name={stackName} className="font-sans text-[15px] font-normal text-foreground no-underline" />, created {at(set.CreationTime)}.{" "}{set.Status === "FAILED" ? `It could not be made: ${(set.StatusReason ?? "no reason was given").replace(/[.\s]+$/, "")}.` : `It previews ${rows.length} ${rows.length === 1 ? "change" : "changes"} and has ${set.ExecutionStatus === "EXECUTE_COMPLETE" ? "been executed" : "not changed anything yet"}.`}</>}
        actions={
          <>
            <Button type="button" onClick={run} disabled={!ready || execute.isPending} title={ready ? undefined : `A change set in execution status ${set.ExecutionStatus} cannot be executed`} data-testid="execute-changeset">{execute.isPending ? "Working…" : "Execute change set"}</Button>
            <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setAsking(true)} data-testid="delete-changeset">Delete change set</Button>
          </>
        }
      />
      {execute.error && <div className="mb-6"><ErrorBlock what="ExecuteChangeSet was refused" error={execute.error} hint={HINT} testId="execute-error" /></div>}
      <div className="grid gap-8">
        <ChangeSetFacts set={set} stack={<StackTo name={stackName} />} />
        <ChangesTable set={set} />
        <div className="grid items-start gap-6 lg:grid-cols-2">
          <ParametersCard set={set} />
          {set.Status !== "FAILED" && <ExecutesCard creating={creating} rows={rows} />}
        </div>
      </div>
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) remove.reset(); }}
        title="Delete change set?"
        description={<>Deleting <b className="font-mono text-[13px] text-bright">{setName}</b> removes the preview. The stack and its resources are not touched.</>}
        verb="Delete change set"
        call="DeleteChangeSet"
        pending={remove.isPending}
        error={remove.error}
        onConfirm={() => { setRemoving(true); remove.mutate({ StackName: stackName, ChangeSetName: setName }, { onSuccess: () => { setAsking(false); toStack({ tab: "changesets" }); }, onError: () => setRemoving(false) }); }}
        testId="delete-changeset-dialog"
      />
    </div>
  );
}