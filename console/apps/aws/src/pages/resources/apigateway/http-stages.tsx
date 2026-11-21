// An HTTP API's Stages (apigateway__http-stages, http-stages-none): the stages with their automatic deployment, the one chosen as details
// (its invoke URL in AWS's form and this doze-aws's, its deployment), Create stage, Deploy for a stage deployed by hand, and Edit.
import { useNavigate } from "@tanstack/react-router";
import { PlusIcon, UploadIcon, PencilIcon, Layers3Icon } from "lucide-react";
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import type { CallRecord } from "../../../api/types/CallRecord";
import { Field, RecentCalls, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { HTTP, useHttpDeployments, useHttpStages, useWrite } from "./api";
import { ApiChips, Card, Mono, Rows, Said, useWhere } from "./parts";
import { HTTP_SECTIONS } from "./sections";
import type { HttpApi, HttpStage } from "./types";
import { httpStagesSentence, invokeUrls, stageNameProblem, when } from "./words";

const ACTIONS = ["GetStage", "GetStages", "CreateStage", "UpdateStage", "DeleteStage", "CreateDeployment", "GetDeployments", "GetDeployment"];

function StageForm({ api, stage, onClose }: { api: string; stage: HttpStage | null; onClose: () => void }) {
  const [name, setName] = useState(stage?.stageName ?? "");
  const [auto, setAuto] = useState(stage?.autoDeploy ?? false);
  const [description, setDescription] = useState(stage?.description ?? "");
  const [tried, setTried] = useState(false);
  const create = useWrite(HTTP, "CreateStage");
  const update = useWrite(HTTP, "UpdateStage");
  const op = stage ? update : create;
  const problem = stage ? null : (name === "$default" ? null : stageNameProblem(name));
  const failed = op.error ? errorWords(op.error) : null;
  const submit = () => {
    setTried(true);
    if (problem) return;
    if (stage) update.mutate({ ApiId: api, StageName: stage.stageName, autoDeploy: auto, description } as never, { onSuccess: onClose });
    else create.mutate({ ApiId: api, stageName: name, autoDeploy: auto, ...(description ? { description } : {}) } as never, { onSuccess: onClose });
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]" data-testid="http-stage-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">{stage ? `Edit stage ${stage.stageName}` : "Create stage"}</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">A stage is a named reference to a deployment of the API.</DialogDescription></DialogHeader>
        <form className="grid gap-5" noValidate onSubmit={(e) => { e.preventDefault(); submit(); }}>
          {!stage && <Field label="Stage name" error={tried ? (problem ?? undefined) : undefined} help={<><span className="font-mono">$default</span> is served from the base of the URL; any other stage is in the path.</>}>{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="$default" className="font-mono" autoComplete="off" spellCheck={false} data-testid="http-stage-name" />}</Field>}
          <div className="flex items-start gap-3"><Switch id="auto" checked={auto} onCheckedChange={setAuto} className="mt-0.5" data-testid="http-stage-auto" /><div className="grid gap-0.5"><Label htmlFor="auto">Automatic deployment</Label><p className="text-[13px] text-muted-foreground">A change to a route, integration or authorizer is released for you.</p></div></div>
          <Field label="Description" optional>{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" data-testid="http-stage-description" />}</Field>
          {failed && <Alert tone="bad" title={`${stage ? "UpdateStage" : "CreateStage"} was refused${failed.code ? `: ${failed.code}` : ""}`} testId="http-stage-error">{failed.message}</Alert>}
          <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button><Button type="submit" disabled={op.isPending} data-testid="http-stage-save">{op.isPending ? "Saving…" : stage ? "Save changes" : "Create stage"}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function HttpStages({ id, api, calls, selected }: { id: string; api: HttpApi; calls: CallRecord[]; selected: string | undefined }) {
  const navigate = useNavigate();
  const { region, endpoint } = useWhere();
  const stages = useHttpStages(id);
  const deployments = useHttpDeployments(id);
  const deploy = useWrite(HTTP, "CreateDeployment");
  const [form, setForm] = useState<{ stage: HttpStage | null } | null>(null);
  const go = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: search as never });
  if (stages.isError) return <ErrorBlock what="The stages could not be read" error={stages.error} hint={HINT} onRetry={() => void stages.refetch()} retrying={stages.isFetching} testId="stages-error" />;
  if (!stages.data) return null;
  const all = stages.data.items ?? [];
  const deps = deployments.data?.items ?? [];
  const chips = <ApiChips kind="apis" id={id} name={api.name} sections={HTTP_SECTIONS} current="stages" />;
  const create = <Button variant="outline" className="bg-card" onClick={() => setForm({ stage: null })} data-testid="create-stage"><PlusIcon /> Create stage</Button>;
  if (all.length === 0) {
    return (
      <>
        {chips}
        <EmptyState icon={Layers3Icon} title="No stages" action={create}>{api.name} has no stage, so it cannot be invoked. A stage is a named reference to a deployment.</EmptyState>
        {form && <StageForm api={id} stage={form.stage} onClose={() => setForm(null)} />}
      </>
    );
  }
  const chosen = all.find((s) => s.stageName === selected) ?? all[0]!;
  const urls = invokeUrls({ id, region, endpoint, stage: chosen.stageName, http: true });
  const dep = deps.find((d) => d.deploymentId === chosen.deploymentId);
  const columns: DataColumn<HttpStage>[] = [
    { id: "name", header: "Stage name", min: 160, accessorFn: (s) => s.stageName, cell: (c) => <span className="font-mono text-[13.5px] font-medium text-bright" data-testid="stage-name-cell">{c.row.original.stageName}</span> },
    { id: "auto", header: "Automatic deployment", size: 230, accessorFn: (s) => (s.autoDeploy ? "On" : "Off") },
    { id: "deployment", header: "Deployment", size: 200, accessorFn: (s) => s.deploymentId ?? "", cell: (c) => (c.row.original.deploymentId ? <span className="font-mono text-[13px]">{c.row.original.deploymentId}</span> : <span className="text-muted-foreground">None</span>) },
  ];
  const failed = deploy.error ? errorWords(deploy.error) : null;
  return (
    <>
      <Said text={httpStagesSentence(api.name, all, deps)} names={[...all.map((s) => s.stageName), ...deps.map((d) => d.deploymentId)]} />
      {chips}
      <div className="mb-3 flex justify-end">{create}</div>
      <DataTable<HttpStage> aria-label="Stages" data={all} columns={columns} rowId={(s) => s.stageName} onRowClick={(s) => go({ tab: "stages", name: s.stageName })} rowClassName={(s) => (s.stageName === chosen.stageName ? "[&>td]:bg-pick/50" : undefined)} />
      <div className="mt-8" />
      <Card title="Stage details" sub={<span className="font-mono">{chosen.stageName}</span>} action={<div className="flex flex-wrap gap-2">{!chosen.autoDeploy && <Button variant="outline" size="sm" className="bg-card" disabled={deploy.isPending} onClick={() => deploy.mutate({ ApiId: id, stageName: chosen.stageName } as never)} data-testid="deploy-stage"><UploadIcon /> Deploy</Button>}<Button variant="outline" size="sm" className="bg-card" onClick={() => setForm({ stage: chosen })} data-testid="edit-stage"><PencilIcon /> Edit</Button></div>} testId="stage-details">
        <Rows rows={[
          { label: "Invoke URL", value: <Mono>{urls.aws}</Mono>, note: <>Also at <span className="font-mono break-all">{urls.local}</span></> },
          { label: "Automatic deployment", value: chosen.autoDeploy ? "On" : "Off" },
          { label: "Deployment", value: chosen.deploymentId ? <><span className="font-mono">{chosen.deploymentId}</span>{dep?.createdDate ? <> {when(dep.createdDate)}{dep.autoDeployed ? ", automatic" : ""}</> : null}</> : "None yet" },
          { label: "Description", value: chosen.description || <span className="text-muted-foreground">–</span> },
        ]} />
        {failed && <div className="mt-4"><Alert tone="bad" title={`CreateDeployment was refused${failed.code ? `: ${failed.code}` : ""}`} testId="deploy-error">{failed.message}</Alert></div>}
        <div className="mt-6 grid gap-3">
          <RuleHelp>The <span className="font-mono">$default</span> stage is served from the base of the URL, with no stage name in the path{all.some((s) => s.stageName !== "$default") ? <>; <span className="font-mono">{all.find((s) => s.stageName !== "$default")!.stageName}</span> answers under <span className="font-mono">/{all.find((s) => s.stageName !== "$default")!.stageName}</span></> : null}.</RuleHelp>
          <RuleHelp>With automatic deployment on, a change to the API is released for you; with it off, you publish with a deployment. doze-aws serves an automatic stage from the live API; a stage deployed by hand is served from its deployment&apos;s snapshot (<span className="font-mono">crates/aws-apigateway/src/plane/find.rs</span>).</RuleHelp>
        </div>
      </Card>
      <RecentCalls className="mt-12" calls={calls.filter((c) => c.action !== null && ACTIONS.includes(c.action))} noun="stage" title="Recent calls" scope={`the stages of ${api.name}`} />
      {form && <StageForm api={id} stage={form.stage} onClose={() => setForm(null)} />}
    </>
  );
}
