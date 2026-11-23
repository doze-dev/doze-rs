// Usage plans (apigateway__rest-usage-plans, usage-plans-none): who may use which stages, and how fast. A plan groups the API keys you hand to
// customers with the stages they may call; doze-aws enforces its throttle and its quota.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, Trash2Icon, GaugeIcon } from "lucide-react";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { PlainConfirm, RuleHelp, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";
import { useMutation } from "@tanstack/react-query";
import { useHttpApis, useForget, usePlans, useRestApis } from "./api";
import { AccountNav, Said } from "./parts";
import { PlanCreate } from "./plan-create";
import type { UsagePlan } from "./types";
import { list, quotaWords, stageWords, throttleWords } from "./words";
import { ResourceTo } from "../value";

export function PlansList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { do?: string; gone?: string };
  if (search.do === "create") return <PlanCreate />;
  return <Plans service={service} kind={kind} gone={search.gone} />;
}

function Plans({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const plans = usePlans();
  const rest = useRestApis();
  const http = useHttpApis();
  const forget = useForget();
  const [deleting, setDeleting] = useState<UsagePlan | null>(null);
  const del = useMutation({ mutationFn: (id: string) => client.api(RESOURCE_PATHS.one(service, kind, id), { method: "DELETE" }) });
  const names = new Map([...(rest.data?.item ?? []).map((a) => [a.id, a.name] as const), ...(http.data?.items ?? []).map((a) => [a.apiId, a.name] as const)]);
  const go = (search: { do?: string; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const all = plans.data?.item ?? [];
  const create = <Button onClick={() => go({ do: "create" })} data-testid="create-open"><PlusIcon /> Create usage plan</Button>;
  const covering = all.flatMap((p) => (p.apiStages ?? []).map((s) => ({ plan: p.name, stage: stageWords(s, names) })));
  const sentence = covering.length === 0 ? "" : `${list([...new Set(all.filter((p) => (p.apiStages ?? []).length > 0).map((p) => p.name))])} cover${all.filter((p) => (p.apiStages ?? []).length > 0).length === 1 ? "s" : ""} ${list(covering.map((c) => c.stage))}. A method that requires an API key is served only with a key from a plan like this one.`;
  const columns: DataColumn<UsagePlan>[] = [
    { id: "name", header: "Name", min: 180, accessorFn: (p) => p.name, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.id }} className="font-mono text-[13.5px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "description", header: "Description", min: 200, accessorFn: (p) => p.description ?? "", cell: (c) => c.row.original.description || <span className="text-muted-foreground">–</span> },
    { id: "throttle", header: "Throttle", size: 210, accessorFn: (p) => throttleWords(p.throttle) },
    { id: "quota", header: "Quota", size: 170, accessorFn: (p) => quotaWords(p.quota) },
    { id: "stages", header: "API stages", size: 220, accessorFn: (p) => (p.apiStages ?? []).map((s) => stageWords(s, names)).join(", "), cell: (c) => ((c.row.original.apiStages ?? []).length > 0 ? <span className="font-mono text-[13px]">{(c.row.original.apiStages ?? []).map((s) => stageWords(s, names)).join(", ")}</span> : <span className="text-muted-foreground">None</span>) },
    { id: "delete", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete usage plan ${c.row.original.name}`} onClick={() => setDeleting(c.row.original)} data-testid={`delete-${c.row.original.name}`}><Trash2Icon /></Button> },
  ];
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="Usage plan deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b>. Its keys stay, no longer limited by it.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader title="Usage plans" crumbs={[...crumbs, { label: "Usage plans", to: "/resources/apigateway/usageplans" }]} sub="Who may use which stages, and how fast. A plan groups the API keys you hand to customers with the stages they may call." refresh={<RefreshControl updatedAt={plans.dataUpdatedAt} fetching={plans.isFetching} onRefresh={() => void plans.refetch()} interval={false} />} actions={create} />
      {plans.isError ? (
        <ErrorBlock what="The usage plans could not be listed" error={plans.error} hint={HINT} onRetry={() => void plans.refetch()} retrying={plans.isFetching} testId="list-error" />
      ) : !plans.data ? (
        <Skeleton className="h-64 rounded-xl" data-testid="list-loading" />
      ) : all.length === 0 ? (
        <>
          <AccountNav current="usageplans" />
          <EmptyState icon={GaugeIcon} title="No usage plans" action={<Button variant="outline" className="bg-card" onClick={() => go({ do: "create" })} data-testid="create-empty"><PlusIcon /> Create usage plan</Button>}>A usage plan sets throttling and quota limits for the API keys that use it.</EmptyState>
        </>
      ) : (
        <>
          <Said text={sentence} names={[...all.map((p) => p.name), ...covering.map((c) => c.stage)]} />
          <AccountNav current="usageplans" />
          <DataTable<UsagePlan> aria-label="Usage plans" data={all} columns={columns} rowId={(p) => p.id} rowHeight={56} />
          <div className="mt-6 grid max-w-3xl gap-3">
            <RuleHelp><b className="font-semibold text-foreground">Enforced here.</b> doze-aws counts the requests of each key and answers 429 over the throttle or the quota.</RuleHelp>
            <RuleHelp>An API key can sit in several plans, but a given key can be in only one plan for each stage of an API.</RuleHelp>
          </div>
        </>
      )}
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete usage plan?" description={<>Delete <b className="font-mono font-semibold text-bright">{deleting?.name}</b>? Its keys stay, but its throttle and quota no longer apply to them.</>} call="DeleteUsagePlan" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate(deleting.id, { onSuccess: () => { forget(deleting.id); setDeleting(null); go({ gone: deleting.name }); } })} testId="plan-delete" />
    </>
  );
}
