// Create stage (apigateway__rest-stage, Create stage): a named URL for a deployment of the API.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection } from "../../../kit";
import { REST, useDeployments, useStages, useWrite } from "./api";
import { newestFirst, stageNameProblem, when } from "./words";
import { sdkShown } from "./calls";

export function StageCreate({ api, apiName, crumbs, onClose }: { api: string; apiName: string; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const deployments = newestFirst(useDeployments(api).data?.item ?? []);
  const stages = useStages(api).data?.item ?? [];
  const [name, setName] = useState("");
  const [pick, setPick] = useState<string | null>(null);
  const [description, setDescription] = useState("");
  const [tried, setTried] = useState(false);
  const make = useWrite(REST, "CreateStage");
  const deployment = pick ?? deployments[0]?.id ?? "";
  const problem = stageNameProblem(name) ?? (stages.some((s) => s.stageName === name) ? `${apiName} already has a stage named ${name}.` : null) ?? (deployment ? null : "Deploy the API first: a stage points at a deployment.");
  const input = useMemo(() => ({ restApiId: api, stageName: name || "<name>", deploymentId: deployment || "<deployment>", ...(description ? { description } : {}) }), [api, name, deployment, description]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  return (
    <FormPage
      shown={sdkShown}
      title="Create stage"
      sub="A stage is a named reference to a deployment. Callers reach the API through the stage's URL."
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Stages", to: `/resources/apigateway/restapis/${api}?tab=stages` }, { label: "Create stage", to: `/resources/apigateway/restapis/${api}?do=create-stage` }]}
      request={{ service: "apigateway", operation: "CreateStage", input }}
      showInput
      refusal={failure}
      banner={failure ? <Alert tone="bad" title="The stage wasn't created." testId="create-refused">CreateStage answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create stage"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: () => onClose(name) }); }}
      testId="stage-create"
    >
      <FormSection title="Stage details" testId="section-details">
        <Field label="Stage name" error={tried ? (problem ?? undefined) : undefined} help={<><b>Stage names</b> hold letters, digits, hyphens and underscores, up to 128 characters.</>}>
          {(id) => <Input id={id} value={name} onChange={(e) => { setName(e.target.value); make.reset(); }} placeholder="beta" autoComplete="off" spellCheck={false} className="font-mono" data-testid="stage-name" />}
        </Field>
        <Field label="Deployment" hint="The deployment the stage serves: a snapshot of the API.">
          {(id) => (
            <Select value={deployment} onValueChange={setPick}>
              <SelectTrigger id={id} className="w-full bg-card" data-testid="stage-deployment"><SelectValue placeholder="No deployment yet" /></SelectTrigger>
              <SelectContent>{deployments.map((d) => <SelectItem key={d.id} value={d.id}><span className="font-mono">{d.id}</span> <span className="text-muted-foreground">{when(d.createdDate)}{d.description ? `, ${d.description}` : ""}</span></SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <Field label="Stage description" optional>{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" data-testid="stage-description" />}</Field>
      </FormSection>
    </FormPage>
  );
}
