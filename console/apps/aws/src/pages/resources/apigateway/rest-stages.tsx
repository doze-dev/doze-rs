// A REST API's Stages (apigateway__rest-stage, rest-stages-none): the stage chosen as its details, its logs and tracing, its variables and
// the deployment history, each with what doze-aws does with it; the stages as chips with Create stage; and Recent calls of the stages.
import { useNavigate } from "@tanstack/react-router";
import { PencilIcon, PlusIcon, UploadIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { cn } from "@doze/ui/lib/utils";
import { Layers3Icon } from "lucide-react";
import type { CallRecord } from "../../../api/types/CallRecord";
import { RecentCalls, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { useDeployments, useStages } from "./api";
import { ApiChips, ApiLink, Card, Dim, Mono, Rows, Said, useWhere } from "./parts";
import { REST_SECTIONS } from "./sections";
import { type EditKind, StageEdit } from "./stage-edit";
import { DEFAULT_BURST, DEFAULT_RATE, detailsOf, levelWords, loggingOf } from "./stage-model";
import type { Deployment, Stage } from "./types";
import { invokeUrls, newestFirst, stageSentence, when } from "./words";

const STAGE_ACTIONS = ["GetStage", "GetStages", "CreateStage", "UpdateStage", "DeleteStage", "CreateDeployment", "GetDeployments", "GetDeployment", "UpdateDeployment", "DeleteDeployment"];
const onOff = (v: boolean) => (v ? "On" : "Off");

export function RestStages({ id, name, stage: asked, calls, region }: { id: string; name: string; stage: string | undefined; calls: CallRecord[]; region: string }) {
  const navigate = useNavigate();
  const { endpoint } = useWhere();
  const stages = useStages(id);
  const deployments = useDeployments(id);
  const [editing, setEditing] = useState<EditKind | null>(null);
  const [tab, setTab] = useState("variables");
  const all = useMemo(() => stages.data?.item ?? [], [stages.data]);
  const deps = useMemo(() => deployments.data?.item ?? [], [deployments.data]);
  const stage = all.find((s) => s.stageName === asked) ?? all[0];
  const go = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "restapis", name: id }, search: search as never });
  if (stages.isError) return <ErrorBlock what="The stages could not be read" error={stages.error} hint={HINT} onRetry={() => void stages.refetch()} retrying={stages.isFetching} testId="stages-error" />;
  if (!stages.data) return null;
  const chips = <ApiChips kind="restapis" id={id} name={name} sections={REST_SECTIONS} current="stages" />;
  if (!stage) {
    return (
      <>
        <Said text={`${name} has never been deployed, so it has no stages. Deploy the API to create one.`} names={[name]} />
        {chips}
        <EmptyState icon={Layers3Icon} title="No stages" action={<Button onClick={() => go({ tab: "stages", dialog: "deploy" })} data-testid="deploy-empty"><UploadIcon /> Deploy API</Button>}>{name} has never been deployed, so it has no stages. Deploy the API to create one.</EmptyState>
      </>
    );
  }
  const urls = invokeUrls({ id, region, endpoint, stage: stage.stageName, http: false });
  const d = detailsOf(stage);
  const lg = loggingOf(stage);
  const active = deps.find((x) => x.id === stage.deploymentId);
  const edit = (k: EditKind, label: string) => <Button variant="outline" size="sm" className="bg-card" onClick={() => setEditing(k)} data-testid={`edit-${k}`} aria-label={label}><PencilIcon /> Edit</Button>;
  const variables = Object.entries(stage.variables ?? {}).map(([k, v]) => ({ name: k, value: v }));
  const varColumns: DataColumn<{ name: string; value: string }>[] = [
    { id: "name", header: "Name", size: 240, accessorFn: (r) => r.name, cell: (c) => <span className="font-mono font-medium text-bright">{c.row.original.name}</span> },
    { id: "value", header: "Value", accessorFn: (r) => r.value, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.value}</span> },
  ];
  const depColumns: DataColumn<Deployment>[] = [
    { id: "id", header: "Deployment ID", size: 190, accessorFn: (x) => x.id, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}{c.row.original.id === stage.deploymentId && <span className="ml-2 rounded-full border bg-pick px-2 py-0.5 font-sans text-[11.5px]">Active</span>}</span> },
    { id: "created", header: "Created", size: 210, accessorFn: (x) => x.createdDate ?? 0, cell: (c) => when(c.row.original.createdDate) },
    { id: "description", header: "Description", accessorFn: (x) => x.description ?? "", cell: (c) => c.row.original.description || <Dim /> },
  ];
  return (
    <>
      <Said text={stageSentence(stage, deps)} names={[stage.stageName, ...deps.map((x) => x.id)]} bold={[/“[^”]*”/]} />
      {chips}
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="grid min-w-0 gap-6">
          <Card title="Stage details" action={edit("details", "Edit stage details")} testId="stage-details">
            <Rows rows={[
              { label: "Stage name", value: <Mono>{stage.stageName}</Mono> },
              { label: "Invoke URL", value: <Mono>{urls.aws}</Mono>, note: <>Also at <span className="font-mono break-all">{urls.local}</span></> },
              { label: "Active deployment", value: active ? <>{when(active.createdDate)} <span className="font-mono text-[13px] text-muted-foreground">{active.id}</span></> : <Dim>None</Dim> },
              { label: "Stage description", value: stage.description || <Dim /> },
              { label: "Provision API cache", value: onOff(d.cache), note: "Stored, not applied locally." },
              { label: "Throttling", value: d.throttle ? `On: ${d.rate} per second, burst ${d.burst}` : "Off", note: d.throttle || (Number(d.rate) === DEFAULT_RATE && Number(d.burst) === DEFAULT_BURST) ? "Stored, not applied locally: nothing is throttled here." : undefined },
              { label: "Web ACL", value: stage.webAclArn ? <Mono>{stage.webAclArn}</Mono> : "Not available in doze-aws: there is no AWS WAF locally." },
              { label: "Client certificate", value: stage.clientCertificateId ? <Mono>{stage.clientCertificateId}</Mono> : "None", note: "Stored, not applied locally: backends are called without a client certificate." },
            ]} />
            <RuleHelp className="mt-6"><b>Stage names</b> hold letters, digits, hyphens and underscores, up to 128 characters. A stage is a named reference to a deployment, which is a snapshot of the API.</RuleHelp>
          </Card>
          <Card title="Logs and tracing" action={edit("logging", "Edit logs and tracing")} testId="stage-logging">
            <Rows rows={[
              { label: "CloudWatch Logs", value: levelWords(lg.level) },
              { label: "Data tracing", value: onOff(lg.dataTrace) },
              { label: "Detailed metrics", value: onOff(lg.metrics), note: lg.metrics ? "Stored, not applied locally: doze-aws emits no API Gateway metrics." : undefined },
              { label: "Custom access logging", value: onOff(lg.access) },
              ...(lg.access ? [{ label: "Access log destination ARN", value: <Mono>{lg.destination}</Mono> }, { label: "Log format", value: <Mono>{lg.format}</Mono> }] : []),
              { label: "X-Ray tracing", value: "Not available in doze-aws: there is no X-Ray locally." },
            ]} />
            <div className="mt-6 grid gap-3">
              <RuleHelp><b>CloudWatch Logs</b> sets how much of each request the execution log records, to <span className="font-mono">API-Gateway-Execution-Logs_{id}/{stage.stageName}</span>. Turning it on needs the account&apos;s CloudWatch Logs role, as on AWS.{lg.access && <> Access logs from this stage go to the log group <span className="font-mono">{lg.destination.split("log-group:")[1] ?? lg.destination}</span>.</>}</RuleHelp>
              <RuleHelp><b>Data tracing</b> writes request and response payloads to the logs and can record sensitive data; AWS recommends it stays off in production.</RuleHelp>
            </div>
          </Card>
          <Tabs value={tab} onValueChange={setTab} className="gap-0">
            <TabsList variant="line" className="h-11 w-full justify-start gap-2 rounded-none border-b p-0" aria-label="Stage variables and deployment history">
              <TabsTrigger value="variables" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-variables">Stage variables</TabsTrigger>
              <TabsTrigger value="history" className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid="tab-history">Deployment history</TabsTrigger>
            </TabsList>
            <div className="pt-6">
              {tab === "variables" ? (
                <Card title="Stage variables" sub="Name and value pairs for this stage. A Lambda proxy integration receives them in the event." action={edit("variables", "Edit stage variables")} testId="stage-variables-card">
                  <DataTable aria-label="Stage variables" data={variables} columns={varColumns} rowId={(r) => r.name} empty="This stage has no variables." />
                  <RuleHelp className="mt-6">Names may hold letters, digits and underscores; a value matches <span className="font-mono">[A-Za-z0-9-._~:/?#&amp;=,]+</span>. A Lambda proxy integration receives them in the event as <span className="font-mono">stageVariables</span>.</RuleHelp>
                </Card>
              ) : (
                <Card title="Deployment history" sub="Every deployment of the API, newest first. The stage points at the active one." testId="stage-history-card">
                  <DataTable aria-label="Deployment history" data={newestFirst(deps)} columns={depColumns} rowId={(x) => x.id} empty="No deployment has been made." />
                </Card>
              )}
            </div>
          </Tabs>
        </div>
        <Card title="Stages" className="xl:sticky xl:top-6" testId="stage-list">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex flex-wrap gap-2">
              {all.map((s: Stage) => (
                <ApiLink key={s.stageName} kind="restapis" id={id} search={{ tab: "stages", name: s.stageName }} className={cn("inline-flex h-9 items-center rounded-full border px-4 font-mono text-[13.5px] no-underline hover:no-underline", s.stageName === stage.stageName ? "border-primary/30 bg-pick font-medium text-bright" : "bg-card text-foreground hover:bg-accent")} testId={`stage-chip-${s.stageName}`}>{s.stageName}</ApiLink>
              ))}
            </div>
            <Button variant="outline" size="sm" className="bg-card" onClick={() => go({ do: "create-stage" })} data-testid="create-stage"><PlusIcon /> Create stage</Button>
          </div>
          <p className="mt-4 text-[13.5px] leading-5 text-muted-foreground">{all.length === 1 ? "One stage. Create another to point a second URL at a deployment." : `${all.length} stages, each a URL for a deployment.`}</p>
        </Card>
      </div>
      <RecentCalls className="mt-12" calls={calls.filter((c) => c.action !== null && STAGE_ACTIONS.includes(c.action))} noun="stage" title="Recent calls" scope={`the stages of ${name}`} />
      {editing && <StageEdit kind={editing} stage={stage} api={id} onClose={() => setEditing(null)} />}
    </>
  );
}
