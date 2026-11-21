// A stack's page (cloudformation__stack-detail and its tabs: events, resources, outputs, parameters, template, change sets; and the
// states that read differently: stack-review-pending, stack-rolled-back, stack-updated). The page of a name CloudFormation does not
// answer to is the deleted stack's (stack-deleted) or says there is none.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, FlowStrip, PlainConfirm, ResourcePage, ResourcePageSkeleton, type Verb } from "../../../kit";
import { HINT } from "../index-page";
import { at } from "../deep/parts";
import type { OwnProps } from "../deep/registry";
import { templateText, useCfnRead, useCfnWrite, useEvents, useImporters, usePoll, useResources, useStack, useTemplate, useWhile } from "./api";
import { DeleteStackDialog } from "./delete-dialog";
import { DeletedStack } from "./deleted";
import { type Stack, inProgress, isGone, resourceTarget, retainedIn } from "./model";
import { Lede, statusBadge } from "./parts";
import { stackSaid } from "./sentences";
import { StackStates } from "./states";
import { type ChangeSetRow, ChangeSetsTab } from "./tab-changesets";
import { EventsTab } from "./tab-events";
import { InfoTab } from "./tab-info";
import { OutputsTab, ParametersTab, ResourcesTab } from "./tab-lists";
import { TemplateTab } from "./tab-template";

const UPDATABLE = ["CREATE_COMPLETE", "UPDATE_COMPLETE", "UPDATE_ROLLBACK_COMPLETE", "IMPORT_COMPLETE", "IMPORT_ROLLBACK_COMPLETE"];
const verb = (id: string, label: string, danger = false): Verb => ({ id, label, danger, op: null, needs: null });

export function StackPage({ service, kind, name }: OwnProps) {
  const nm = name ?? "";
  const detail = useResourceDetail(service, kind, nm);
  if (detail.isError && isGone(detail.error)) return <DeletedStack name={nm} />;
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={nm} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <StackView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refetch={() => void detail.refetch()} />;
}

function StackView({ d, updatedAt, fetching, refetch }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refetch: () => void }) {
  const nm = d.name;
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const [asked, setTab] = useUrlParam("tab");
  const [deleting, setDeleting] = useState(false);
  const [protecting, setProtecting] = useState(false);
  const protect = useCfnWrite<unknown>("UpdateTerminationProtection");
  const stackQ = useStack(nm);
  const stack: Stack | undefined = stackQ.data?.Stacks?.[0];
  const status = stack?.StackStatus ?? "";
  const resources = useResources(nm, Boolean(stack));
  const events = useEvents(nm, undefined, status === "ROLLBACK_COMPLETE");
  const exportNames = (stack?.Outputs ?? []).flatMap((o) => (o.ExportName ? [o.ExportName] : []));
  const importers = useImporters(exportNames);
  const template = useTemplate(nm, "Original", deleting || asked === "resources");
  const del = useDelete(d.service, d.kind);
  const view = status === "REVIEW_IN_PROGRESS" ? "review" : status === "ROLLBACK_COMPLETE" ? "rolled" : search.done === "updated" && status === "UPDATE_COMPLETE" ? "updated" : "tabs";
  const sets = useCfnRead<{ Summaries?: ChangeSetRow[] }>("ListChangeSets", { StackName: nm }, { enabled: view === "review" });
  const reviewSet = sets.data?.Summaries?.[0]?.ChangeSetName;
  const execute = useCfnWrite<unknown>("ExecuteChangeSet");
  const dropSet = useCfnWrite<unknown>("DeleteChangeSet");
  const refresh = () => {
    refetch();
    void stackQ.refetch();
    void resources.refetch();
  };
  // Just after a write (`?done=`) the page reads again for a while, for the work it started has not begun to show yet.
  const recent = useWhile(Boolean(search.done), 15_000);
  usePoll(inProgress(status) || recent, refresh);
  if (stackQ.isError && isGone(stackQ.error)) return <DeletedStack name={nm} />;
  if (!stack) return stackQ.isError ? <ErrorBlock what="This stack could not be read" error={stackQ.error} hint={HINT} onRetry={refresh} retrying={stackQ.isFetching} testId="detail-error" /> : <ResourcePageSkeleton />;
  const rows = resources.data?.StackResourceSummaries ?? [];
  const to = (search2: Record<string, string>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: nm }, search: search2 });
  const update = (mode?: string) => to({ do: "update", ...(mode ? { mode } : {}) });
  const updatable = UPDATABLE.includes(status);
  const verbs = view === "rolled" ? [verb("delete", "Delete", true)] : view === "review" ? (reviewSet ? [verb("execute", "Execute change set"), verb("dropset", "Delete change set", true)] : [verb("delete", "Delete", true)]) : [verb("update", "Update"), verb("delete", "Delete", true)];
  const shown: ResourceDetail = { ...d, badges: [statusBadge(status)], sentence: null, wired: [], verbs };
  const said = stackSaid({ name: nm, status, reason: stack.StackStatusReason, created: stack.CreationTime ? at(stack.CreationTime) : null, updated: stack.LastUpdatedTime ? at(stack.LastUpdatedTime) : null, resources: rows, exports: exportNames, importers: importers.data ?? {}, failed: null });
  const owned = rows.filter((r) => r.ResourceStatus !== "DELETE_COMPLETE");
  const strip = owned.length > 0 && (
    <FlowStrip groups={[{ label: "Owns", chips: [{ label: nm, self: true }] }, { chips: owned.map((r) => ({ label: resourceTarget(r)?.name ?? r.LogicalResourceId, to: resourceTarget(r) ?? undefined, title: `${r.LogicalResourceId} (${r.ResourceType})` })) }]} />
  );
  const created = stack.CreationTime ? at(stack.CreationTime) : "";
  const rolledAt = events.data?.StackEvents?.[0]?.Timestamp;
  const meta = status === "ROLLBACK_COMPLETE" ? `Created ${created}.${rolledAt ? ` Rolled back ${at(rolledAt)}.` : ""}` : status === "REVIEW_IN_PROGRESS" ? `Created ${created}. It has no resources yet: it waits for you to execute its change set.` : stack.LastUpdatedTime ? `Created ${created}. Updated ${at(stack.LastUpdatedTime)}.` : `Created ${created}. Not updated since.`;
  const onVerb = (v: Verb) => {
    if (v.id === "update") update();
    else if (v.id === "delete") setDeleting(true);
    else if (v.id === "execute" && reviewSet) execute.mutate({ StackName: nm, ChangeSetName: reviewSet }, { onSuccess: () => to({ tab: "events", done: "executed" }) });
    else if (v.id === "dropset" && reviewSet) dropSet.mutate({ StackName: nm, ChangeSetName: reviewSet }, { onSuccess: () => { void sets.refetch(); refresh(); } });
  };
  const banner =
    search.done === "created" ? <ResultBanner title={`Stack ${nm} is being created.`} returned="CreateStack returned its stack ID. The stack is made in the background; this page follows it until it settles." onDismiss={() => to({ tab: "events" })} testId="created-banner" />
    : search.done === "executed" ? <ResultBanner title="Change set executed." returned="ExecuteChangeSet was accepted. The stack is built or changed in the background; this page follows it until it settles." onDismiss={() => to({ tab: "events" })} testId="executed-banner" />
    : search.done === "updated" && view === "tabs" ? <ResultBanner tone={inProgress(status) ? "info" : "warn"} title={inProgress(status) ? `Stack ${nm} is being updated.` : `The update of ${nm} did not complete.`} returned={inProgress(status) ? "UpdateStack was accepted. The update runs in the background; this page follows it until it settles." : `The stack is ${status}: its events say why.`} onDismiss={() => to({ tab: "events" })} testId="updating-banner" />
    : search.done === "deleting" ? <ResultBanner tone="info" title={`Stack ${nm} is being deleted.`} returned="DeleteStack was accepted. The stack is DELETE_IN_PROGRESS until its resources are gone." onDismiss={() => to({})} testId="deleting-banner" />
    : undefined;
  const states = view !== "tabs" && <StackStates view={view} stack={stack} d={d} set={reviewSet} />;
  const tabs = [
    { id: "info", label: "Stack info", render: () => <InfoTab stack={stack} d={d} /> },
    { id: "events", label: "Events", render: () => <EventsTab name={nm} /> },
    { id: "resources", label: "Resources", render: () => <ResourcesTab name={nm} /> },
    { id: "outputs", label: "Outputs", render: () => <OutputsTab stack={stack} importers={importers.data} /> },
    { id: "parameters", label: "Parameters", render: () => <ParametersTab stack={stack} /> },
    { id: "template", label: "Template", render: () => <TemplateTab name={nm} /> },
    { id: "changesets", label: "Change sets", render: () => <ChangeSetsTab name={nm} makeOne={() => update("changeset")} updatable={updatable} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={meta}
        banner={banner}
        overview={view === "tabs" ? ((asked ?? "info") === "info" ? <><Lede said={said} />{strip}</> : undefined) : states}
        tabs={view === "tabs" ? tabs : []}
        details={false}
        tab={asked ?? "info"}
        onTab={(id) => setTab(id === "info" ? undefined : id)}
        primary={view === "review" ? "execute" : "update"}
        actions={stack.ParentId ? [] : [{ label: stack.EnableTerminationProtection ? "Turn off termination protection" : "Turn on termination protection", onSelect: () => setProtecting(true) }]}
        verbStates={{ update: { disabled: updatable ? undefined : `a stack in ${status} cannot be updated`, quiet: true } }}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="stack-page"
      />
      <PlainConfirm
        open={protecting}
        onOpenChange={(o) => { setProtecting(o); if (!o) protect.reset(); }}
        title={stack.EnableTerminationProtection ? "Turn off termination protection?" : "Turn on termination protection?"}
        description={stack.EnableTerminationProtection ? <>DeleteStack on <b className="font-mono text-[13px] text-bright">{nm}</b> will be carried out again.</> : <>DeleteStack on <b className="font-mono text-[13px] text-bright">{nm}</b> will fail and leave the stack as it is, until you turn it off.</>}
        verb={stack.EnableTerminationProtection ? "Turn off" : "Turn on"}
        call="UpdateTerminationProtection"
        pending={protect.isPending}
        error={protect.error}
        onConfirm={() => protect.mutate({ StackName: nm, EnableTerminationProtection: !stack.EnableTerminationProtection }, { onSuccess: () => { setProtecting(false); refresh(); } })}
        testId="protection-dialog"
      />
      <DeleteStackDialog
        open={deleting}
        onOpenChange={(o) => { setDeleting(o); if (!o) del.reset(); }}
        stack={stack}
        resources={resources.data ? rows : undefined}
        retained={retainedIn(templateText(template.data?.TemplateBody))}
        importers={importers.isPending && exportNames.length > 0 ? undefined : (importers.data ?? {})}
        pending={del.isPending}
        error={del.error}
        onConfirm={() => del.mutate(nm, { onSuccess: () => { setDeleting(false); to({ done: "deleting" }); } })}
      />
    </>
  );
}
