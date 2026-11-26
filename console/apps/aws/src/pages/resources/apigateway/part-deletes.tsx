// The deletes of a part of an API (apigateway: a REST API's stage, deployment, resource and method; an HTTP API's stage and deployment):
// each asks for the name to be typed, says in words what goes with it and what stays, and names whatever the service would refuse it for
// (a deployment a stage serves). Each is one call as the caller `console`.
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import type { Kind } from "../../../kit";
import { HTTP, REST, useWrite } from "./api";
import { deploymentBlocker, methodImpact, plansListing, resourceImpact, stagesServing } from "./delete-model";
import type { Deployment, HttpDeployment, HttpStage, RestMethod, RestResource, Stage, UsagePlan } from "./types";
import { list, when } from "./words";

const bold = "font-semibold text-bright";
const mono = `font-mono ${bold}`;

type Shared = { open: boolean; onOpenChange: (o: boolean) => void; onDone: () => void };

/** One delete, as a typed confirmation. `word` is what is typed; `blocked` keeps the button off (the page says why in `children`). */
function PartDelete({ open, onOpenChange, onDone, kind, operation, input, title, word, noun, description, blocked = false, children, testId }: Shared & { kind: Kind; operation: string; input: object; title: string; word: string; noun: string; description: React.ReactNode; blocked?: boolean; children?: React.ReactNode; testId: string }) {
  const del = useWrite(kind, operation);
  const failed = del.error ? errorWords(del.error) : null;
  return (
    <TypedConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
      title={title}
      description={description}
      words={word}
      noun={noun}
      verb={title.replace(/\?$/, "")}
      pending={del.isPending}
      disabled={blocked}
      refusal={failed && <Alert tone="bad" title={`${operation} was refused${failed.code ? `: ${failed.code}` : ""}`} testId={`${testId}-refused`}>{failed.message}</Alert>}
      onConfirm={() => del.mutate(input as never, { onSuccess: onDone })}
      testId={testId}
    >
      {children}
    </TypedConfirm>
  );
}

/** Delete stage (REST): its invoke URL stops answering; the deployment it served stays in the history. */
export function RestStageDelete({ api, apiName, stage, url, plans, ...rest }: Shared & { api: string; apiName: string; stage: Stage; url: string; plans: readonly UsagePlan[] }) {
  const listed = plansListing(plans, api, stage.stageName);
  return (
    <PartDelete {...rest} kind={REST} operation="DeleteStage" input={{ restApiId: api, stageName: stage.stageName }} title="Delete stage?" word={stage.stageName} noun="stage name" testId="stage-delete"
      description={<>Delete the stage <b className={mono}>{stage.stageName}</b> of <b className={mono}>{apiName}</b>? Its invoke URL <span className="font-mono break-all">{url}</span> stops answering.</>}>
      <div className="grid gap-3 text-[13.5px] leading-[21px]">
        <p data-testid="stage-delete-stays">{stage.deploymentId ? <>The deployment <span className="font-mono">{stage.deploymentId}</span> stays in the deployment history, and a new stage can point at it.</> : "The stage has no deployment."}</p>
        {listed.length > 0 && <p data-testid="stage-delete-plans">The usage {listed.length === 1 ? "plan" : "plans"} <b className={mono}>{list(listed)}</b> {listed.length === 1 ? "lists" : "list"} <span className="font-mono">{apiName}:{stage.stageName}</span> and keep{listed.length === 1 ? "s" : ""} listing it: DeleteStage does not touch a plan.</p>}
      </div>
    </PartDelete>
  );
}

/** Delete deployment (REST): refused while a stage serves it, which the dialog says in words and does not offer. */
export function RestDeploymentDelete({ api, deployment, stages, ...rest }: Shared & { api: string; deployment: Deployment; stages: readonly Stage[] }) {
  const serving = stagesServing(stages, deployment.id);
  const why = deploymentBlocker(serving);
  return (
    <PartDelete {...rest} kind={REST} operation="DeleteDeployment" input={{ restApiId: api, deploymentId: deployment.id }} title="Delete deployment?" word={deployment.id} noun="deployment ID" blocked={why !== null} testId="deployment-delete"
      description={<>Delete the deployment <b className={mono}>{deployment.id}</b>{deployment.createdDate ? <>, made {when(deployment.createdDate)}</> : null}? It leaves the deployment history, and no stage can be pointed at it again.</>}>
      {why && <Callout tone="warn" lead="A stage serves it." testId="deployment-delete-blocker">{why}</Callout>}
    </PartDelete>
  );
}

/** Delete resource (REST): the resources below it and every method of them go with it. */
export function ResourceDelete({ api, resources, resource, ...rest }: Shared & { api: string; resources: readonly RestResource[]; resource: RestResource }) {
  const impact = resourceImpact(resources, resource.id);
  return (
    <PartDelete {...rest} kind={REST} operation="DeleteResource" input={{ restApiId: api, resourceId: resource.id }} title="Delete resource?" word={resource.path} noun="path" blocked={impact === null} testId="resource-delete"
      description={<>Delete the resource <b className={mono}>{resource.path}</b>? {impact?.words}</>}>
      <p className="text-[13.5px] leading-[21px]" data-testid="resource-delete-stage">A stage serves the snapshot its deployment took, so callers keep reaching what is deployed until you deploy the API again.</p>
    </PartDelete>
  );
}

/** Delete method (REST): its integration and the responses of both go with it. */
export function MethodDelete({ api, resource, method, ...rest }: Shared & { api: string; resource: Pick<RestResource, "id" | "path">; method: RestMethod }) {
  return (
    <PartDelete {...rest} kind={REST} operation="DeleteMethod" input={{ restApiId: api, resourceId: resource.id, httpMethod: method.httpMethod }} title="Delete method?" word={method.httpMethod} noun="method" testId="method-delete"
      description={<>Delete <b className={mono}>{method.httpMethod} {resource.path}</b>? {methodImpact(method)}</>}>
      <p className="text-[13.5px] leading-[21px]" data-testid="method-delete-stage">A stage serves the snapshot its deployment took, so callers keep reaching the method until you deploy the API again.</p>
    </PartDelete>
  );
}

/** Delete stage (HTTP and WebSocket): its invoke URL stops answering. */
export function HttpStageDelete({ api, apiName, stage, url, ...rest }: Shared & { api: string; apiName: string; stage: HttpStage; url: string }) {
  return (
    <PartDelete {...rest} kind={HTTP} operation="DeleteStage" input={{ ApiId: api, StageName: stage.stageName }} title="Delete stage?" word={stage.stageName} noun="stage name" testId="stage-delete"
      description={<>Delete the stage <b className={mono}>{stage.stageName}</b> of <b className={mono}>{apiName}</b>? Its invoke URL <span className="font-mono break-all">{url}</span> stops answering.</>}>
      <p className="text-[13.5px] leading-[21px]" data-testid="stage-delete-stays">{stage.deploymentId ? <>The deployment <span className="font-mono">{stage.deploymentId}</span> stays in the deployment history.</> : "The stage has no deployment."}</p>
    </PartDelete>
  );
}

/** Delete deployment (HTTP and WebSocket): refused while a stage serves it. */
export function HttpDeploymentDelete({ api, deployment, stages, ...rest }: Shared & { api: string; deployment: HttpDeployment; stages: readonly HttpStage[] }) {
  const why = deploymentBlocker(stagesServing(stages, deployment.deploymentId));
  return (
    <PartDelete {...rest} kind={HTTP} operation="DeleteDeployment" input={{ ApiId: api, DeploymentId: deployment.deploymentId }} title="Delete deployment?" word={deployment.deploymentId} noun="deployment ID" blocked={why !== null} testId="deployment-delete"
      description={<>Delete the deployment <b className={mono}>{deployment.deploymentId}</b>{deployment.createdDate ? <>, made {when(deployment.createdDate)}</> : null}? It leaves the deployment history.</>}>
      {why && <Callout tone="warn" lead="A stage serves it." testId="deployment-delete-blocker">{why}</Callout>}
    </PartDelete>
  );
}
