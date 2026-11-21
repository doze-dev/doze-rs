// A usage plan's page (apigateway__rest-usage-plan, usage-plan-keys-none): its stages, throttle and quota, and two tabs: the stages it
// covers and the API keys in it, with Add API key (one that exists, or a new one in a step).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { KeyRoundIcon } from "lucide-react";
import { useResourceDetail } from "../../../api/resources";
import { DetailError, Field, PlainConfirm, RuleHelp, Segmented, useServiceCrumbs } from "../../../kit";
import { callKindOp } from "../../../kit";
import { PLANS, useApiKeys, useHttpApis, usePlanKeys, usePlans, useRefreshReads, useRestApis, useStages, useWrite } from "./api";
import { Card, Mono, Rows, Said } from "./parts";
import { addStagePatch, keyProblem, removeStagePatch } from "./plan-model";
import type { PlanKey, UsagePlan } from "./types";
import { count, list, quotaWords, stageWords, throttleWords } from "./words";

export function PlanDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const plans = usePlans();
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  const plan = plans.data?.item?.find((p) => p.id === name);
  if (!detail.data || !plans.data) return <div className="grid gap-4" data-testid="resource-page-loading"><Skeleton className="h-12 w-80" /><Skeleton className="h-72 rounded-[14px]" /></div>;
  if (!plan) return <DetailError service={service} kind={kind} name={name} error={Object.assign(new Error("NotFoundException"), { status: 404, code: "NotFoundException" })} />;
  return <PlanView plan={plan} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={() => { void detail.refetch(); void plans.refetch(); }} />;
}

function AddKey({ plan, have, onClose }: { plan: UsagePlan; have: string[]; onClose: (added: string) => void }) {
  const refresh = useRefreshReads();
  const keys = useApiKeys().data?.item ?? [];
  const free = keys.filter((k) => !have.includes(k.id));
  const [mode, setMode] = useState<"existing" | "new">(free.length > 0 ? "existing" : "new");
  const [pick, setPick] = useState("");
  const [newName, setNewName] = useState("");
  const [tried, setTried] = useState(false);
  const [state, setState] = useState<{ working: boolean; error: unknown }>({ working: false, error: null });
  const chosen = pick || free[0]?.id || "";
  const problem = mode === "new" ? keyProblem(newName) : chosen === "" ? "Choose a key." : null;
  const submit = async () => {
    setTried(true);
    if (problem) return;
    setState({ working: true, error: null });
    try {
      let id = chosen;
      let label = keys.find((k) => k.id === chosen)?.name ?? chosen;
      if (mode === "new") { const made = await callKindOp<{ id: string }>(PLANS, "CreateApiKey", { name: newName.trim(), enabled: true }); id = made.id; label = newName.trim(); }
      await callKindOp(PLANS, "CreateUsagePlanKey", { usagePlanId: plan.id, keyId: id, keyType: "API_KEY" });
      refresh();
      onClose(label);
    } catch (e) { refresh(); setState({ working: false, error: e }); }
  };
  const failed = state.error ? errorWords(state.error) : null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose("")}>
      <DialogContent className="sm:max-w-[560px]" data-testid="add-key-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Add API key</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">Add an existing key to <span className="font-mono">{plan.name}</span>, or create a new one and add it in one step.</DialogDescription></DialogHeader>
        <form className="grid gap-5" noValidate onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <Segmented<"existing" | "new"> value={mode} onChange={setMode} label="Which key" testId="key-mode" options={[{ value: "existing", label: "Existing key" }, { value: "new", label: "New key" }]} className="w-fit" />
          {mode === "existing" ? (
            <Field label="API key" error={tried ? (problem ?? undefined) : undefined} hint={free.length === 0 ? "Every key of the account is in this plan already. Make a new one." : "A given key can be in only one plan for each stage of an API."}>
              {(id) => (
                <Select value={chosen} onValueChange={setPick} disabled={free.length === 0}>
                  <SelectTrigger id={id} className="w-full bg-card" data-testid="key-pick"><SelectValue placeholder="No other key" /></SelectTrigger>
                  <SelectContent>{free.map((k) => <SelectItem key={k.id} value={k.id}><span className="font-mono">{k.name ?? k.id}</span> <span className="text-muted-foreground">{k.id}</span></SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          ) : (
            <Field label="Name of the new key" error={tried ? (problem ?? undefined) : undefined} help="The key is enabled, and its value is made for it. A client sends the value in the x-api-key header.">{(id) => <Input id={id} value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="storefront-paris" className="font-mono" autoComplete="off" spellCheck={false} data-testid="key-name" />}</Field>
          )}
          {failed && <Alert tone="bad" title={`The key was not added: ${failed.code || "refused"}`} testId="add-key-error">{failed.message}</Alert>}
          <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={() => onClose("")}>Cancel</Button><Button type="submit" disabled={state.working} data-testid="key-add-submit">{state.working ? "Adding…" : "Add API key"}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function AddStage({ plan, onClose }: { plan: UsagePlan; onClose: () => void }) {
  const rest = useRestApis().data?.item ?? [];
  const [api, setApi] = useState("");
  const [stage, setStage] = useState("");
  const save = useWrite(PLANS, "UpdateUsagePlan");
  const chosen = api || rest[0]?.id || "";
  const stages = (useStages(chosen).data?.item ?? []).filter((s) => !(plan.apiStages ?? []).some((x) => x.apiId === chosen && x.stage === s.stageName));
  const st = stage && stages.some((s) => s.stageName === stage) ? stage : stages[0]?.stageName ?? "";
  const failed = save.error ? errorWords(save.error) : null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]" data-testid="add-stage-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Add API stage</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">The plan&apos;s keys may then call this stage, and its limits apply to them.</DialogDescription></DialogHeader>
        <form className="grid gap-5" onSubmit={(e) => { e.preventDefault(); if (chosen && st) save.mutate({ usagePlanId: plan.id, patchOperations: addStagePatch(chosen, st) } as never, { onSuccess: onClose }); }}>
          <Field label="REST API" hint={rest.length === 0 ? "There is no REST API yet: usage plans cover the stages of REST APIs." : undefined}>
            {(id) => <Select value={chosen} onValueChange={(v) => { setApi(v); setStage(""); }}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="stage-api"><SelectValue placeholder="No REST API" /></SelectTrigger><SelectContent>{rest.map((a) => <SelectItem key={a.id} value={a.id} className="font-mono">{a.name}</SelectItem>)}</SelectContent></Select>}
          </Field>
          <Field label="Stage" hint={stages.length === 0 && chosen ? "This API has no stage the plan does not cover." : undefined}>
            {(id) => <Select value={st} onValueChange={setStage} disabled={stages.length === 0}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="stage-pick"><SelectValue placeholder="No stage" /></SelectTrigger><SelectContent>{stages.map((s) => <SelectItem key={s.stageName} value={s.stageName} className="font-mono">{s.stageName}</SelectItem>)}</SelectContent></Select>}
          </Field>
          {failed && <Alert tone="bad" title={`UpdateUsagePlan was refused${failed.code ? `: ${failed.code}` : ""}`} testId="add-stage-error">{failed.message}</Alert>}
          <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button><Button type="submit" disabled={!chosen || !st || save.isPending} data-testid="stage-add-submit">{save.isPending ? "Adding…" : "Add stage"}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PlanView({ plan, updatedAt, fetching, refresh }: { plan: UsagePlan; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const search = useSearch({ strict: false }) as { tab?: string; done?: string; name?: string };
  const keys = usePlanKeys(plan.id);
  const rest = useRestApis().data?.item ?? [];
  const http = useHttpApis().data?.items ?? [];
  const names = new Map([...rest.map((a) => [a.id, a.name] as const), ...http.map((a) => [a.apiId, a.name] as const)]);
  const [adding, setAdding] = useState<"key" | "stage" | null>(null);
  const [removing, setRemoving] = useState<{ kind: "key"; key: PlanKey } | { kind: "stage"; api: string; stage: string } | null>(null);
  const delKey = useWrite(PLANS, "DeleteUsagePlanKey");
  const delStage = useWrite(PLANS, "UpdateUsagePlan");
  const tab = search.tab === "keys" ? "keys" : "stages";
  const go = (s: { tab?: string; done?: string; name?: string }) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "usageplans", name: plan.id }, search: s as never });
  const all = keys.data?.item ?? [];
  const stages = plan.apiStages ?? [];
  const stageText = stages.map((s) => stageWords(s, names));
  const sentence = `${plan.name} ${stages.length === 0 ? "covers no stage yet" : `covers ${list(stageText)}`}, and holds ${count(all.length, "API key")}. A method that requires an API key is served only with one of ${all.length === 1 ? "it" : "them"}.`;
  const keyColumns: DataColumn<PlanKey>[] = [
    { id: "name", header: "Name", min: 200, accessorFn: (k) => k.name ?? "", cell: (c) => <span className="font-mono font-medium text-bright" data-testid="key-name-cell">{c.row.original.name ?? c.row.original.id}</span> },
    { id: "id", header: "ID", size: 220, accessorFn: (k) => k.id, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}</span> },
    { id: "remove", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Remove key ${c.row.original.name ?? c.row.original.id}`} onClick={() => setRemoving({ kind: "key", key: c.row.original })} data-testid={`remove-${c.row.original.name ?? c.row.original.id}`}><Trash2Icon /></Button> },
  ];
  const stageColumns: DataColumn<{ apiId: string; stage: string }>[] = [
    { id: "api", header: "API", min: 200, accessorFn: (s) => names.get(s.apiId) ?? s.apiId, cell: (c) => <span className="font-mono font-medium text-bright">{names.get(c.row.original.apiId) ?? c.row.original.apiId}</span> },
    { id: "stage", header: "Stage", size: 220, accessorFn: (s) => s.stage, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.stage}</span> },
    { id: "remove", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Remove stage ${stageWords(c.row.original, names)}`} onClick={() => setRemoving({ kind: "stage", api: c.row.original.apiId, stage: c.row.original.stage })} data-testid={`remove-${c.row.original.stage}`}><Trash2Icon /></Button> },
  ];
  const added = search.done === "key" ? <ResultBanner className="mb-8" title={`API key ${search.name} added.`} returned="CreateUsagePlanKey put it in the plan: its limits apply to it from now on." onDismiss={() => go({ tab: "keys" })} testId="key-banner" /> : search.done === "created" ? <ResultBanner className="mb-8" title={`Usage plan ${plan.name} created.`} returned="CreateUsagePlan returned its ID. Add a stage and some API keys." onDismiss={() => go({})} testId="created-banner" /> : null;
  const err = delKey.error ?? delStage.error;
  return (
    <div data-testid="plan-page">
      <PageHeader title={plan.name} mono crumbs={[...crumbs, { label: "Usage plans", to: "/resources/apigateway/usageplans" }]} titleExtra={<Badge variant="secondary" className="font-normal">Usage plan</Badge>} sub={plan.description ?? <>Created with no description.</>} refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={() => { refresh(); void keys.refetch(); }} interval={false} />} />
      {added}
      <Said text={sentence} names={[plan.name, ...stageText]} bold={[/\d+ API keys?/]} />
      <Card title="Plan details" testId="plan-details">
        <Rows rows={[
          { label: "Associated stages", value: stages.length > 0 ? <Mono>{stageText.join(", ")}</Mono> : "None" },
          { label: "Throttling", value: plan.throttle?.rateLimit === undefined ? "Not set" : `${plan.throttle.rateLimit.toLocaleString("en-US")} requests per second, burst ${(plan.throttle.burstLimit ?? 0).toLocaleString("en-US")}`, note: plan.throttle?.rateLimit === undefined ? undefined : "Enforced: a request over it gets 429." },
          { label: "Quota", value: plan.quota?.limit === undefined ? "Not set" : `${plan.quota.limit.toLocaleString("en-US")} requests per ${(plan.quota.period ?? "month").toLowerCase()}`, note: plan.quota?.limit === undefined ? undefined : "Enforced: once the quota is used, a request gets 429." },
        ]} />
        <RuleHelp className="mt-5">{throttleWords(plan.throttle)}; {quotaWords(plan.quota)}. doze-aws counts per key (<span className="font-mono">crates/aws-apigateway/src/usage.rs</span>).</RuleHelp>
      </Card>
      <Tabs value={tab} onValueChange={(v) => go({ tab: v })} className="mt-10 gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-4 rounded-none border-b p-0" aria-label="Usage plan pages">
          <TabsTrigger value="stages" className="h-11 flex-none px-2 text-[14.5px] after:bottom-[-1px]" data-testid="tab-stages">Associated stages</TabsTrigger>
          <TabsTrigger value="keys" className="h-11 flex-none px-2 text-[14.5px] after:bottom-[-1px]" data-testid="tab-keys">Associated API keys<span className="ml-2 rounded-full bg-muted px-1.5 text-[11.5px] text-muted-foreground tabular-nums" data-testid="keys-count">{keys.data ? all.length : "…"}</span></TabsTrigger>
        </TabsList>
      </Tabs>
      <div className="pt-6">
        {tab === "keys" ? (
          <Card title="Associated API keys" sub="Keys that may call the stages in this plan." action={<Button variant="outline" className="bg-card" onClick={() => setAdding("key")} data-testid="add-key"><PlusIcon /> Add API key</Button>} testId="plan-keys">
            {keys.data && all.length === 0 ? (
              <EmptyState icon={KeyRoundIcon} title="No API keys">No API key is associated with {plan.name}, so its limits apply to nobody yet.</EmptyState>
            ) : (
              <>
                <DataTable<PlanKey> aria-label="Associated API keys" data={all} columns={keyColumns} rowId={(k) => k.id} />
                <RuleHelp className="mt-5">Add an existing key, or create a new one and add it in one step. A key reaches a method in the <span className="font-mono">x-api-key</span> header; a request whose key is missing, disabled or not in a plan that covers the stage gets 403 Forbidden. A given key can be in only one plan for each stage of an API.</RuleHelp>
              </>
            )}
          </Card>
        ) : (
          <Card title="Associated stages" sub="The stages whose callers this plan limits." action={<Button variant="outline" className="bg-card" onClick={() => setAdding("stage")} data-testid="add-stage"><PlusIcon /> Add API stage</Button>} testId="plan-stages">
            {stages.length === 0 ? <p className="text-[14px] text-muted-foreground" data-testid="no-stages">{plan.name} covers no stage yet, so it limits nothing. Add the stage of a REST API.</p> : <DataTable aria-label="Associated stages" data={stages} columns={stageColumns} rowId={(s) => `${s.apiId}:${s.stage}`} />}
          </Card>
        )}
      </div>
      {adding === "key" && <AddKey plan={plan} have={all.map((k) => k.id)} onClose={(label) => { setAdding(null); if (label) go({ tab: "keys", done: "key", name: label }); }} />}
      {adding === "stage" && <AddStage plan={plan} onClose={() => setAdding(null)} />}
      <PlainConfirm
        open={removing !== null}
        onOpenChange={(o) => { if (!o) { setRemoving(null); delKey.reset(); delStage.reset(); } }}
        title={removing?.kind === "key" ? "Remove API key?" : "Remove stage?"}
        verb="Remove"
        description={removing?.kind === "key" ? <>Take <b className="font-mono font-semibold text-bright">{removing.key.name ?? removing.key.id}</b> out of {plan.name}? The key stays, no longer limited by this plan.</> : removing ? <>Take <b className="font-mono font-semibold text-bright">{stageWords({ apiId: removing.api, stage: removing.stage }, names)}</b> out of {plan.name}? Its keys can no longer call it through this plan.</> : undefined}
        call={removing?.kind === "key" ? "DeleteUsagePlanKey" : "UpdateUsagePlan"}
        pending={delKey.isPending || delStage.isPending}
        error={err}
        onConfirm={() => {
          if (removing?.kind === "key") delKey.mutate({ usagePlanId: plan.id, keyId: removing.key.id } as never, { onSuccess: () => setRemoving(null) });
          else if (removing) delStage.mutate({ usagePlanId: plan.id, patchOperations: removeStagePatch(removing.api, removing.stage) } as never, { onSuccess: () => setRemoving(null) });
        }}
        testId="plan-remove"
      />
    </div>
  );
}
