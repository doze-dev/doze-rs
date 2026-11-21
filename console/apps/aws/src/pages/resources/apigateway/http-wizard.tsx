// Create HTTP API (apigateway__http-create, http-create-routes, http-create-stages, http-create-review): four steps (the integration and the
// API, the route, the stage, the review). Nothing is sent until the last step: CreateApi, CreateIntegration, CreateRoute, CreateStage.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, FunctionPicker, RadioCards, RuleHelp, Segmented, useServiceCrumbs } from "../../../kit";
import { useResourceDetail } from "../../../api/resources";
import { HTTP, runCalls, useHttpApis, useRefreshReads, useRestApis } from "./api";
import { type Wizard, ROUTE_METHODS, defaultPath, emptyWizard, wizardCalls, wizardProblem } from "./http-model";
import { useWhere } from "./parts";
import { callKindOp } from "../../../kit";
import { invokeUrls, list } from "./words";

const STEPS = [{ id: "integration", label: "Integrations" }, { id: "routes", label: "Configure routes" }, { id: "stage", label: "Stage" }, { id: "review", label: "Review and create" }];
const TITLES = [["Create HTTP API", "Step 1 of 4. Add the backend, then name the API. The console builds a route and a stage from it, which you review in the next steps."], ["Configure routes", "Step 2 of 4. Review the route the console made for your integration. A route is an HTTP method and a path; it sends matching requests to an integration."], ["Stage", "Step 3 of 4. Review the stage the console made. A stage is a named reference to a deployment of the API, and callers reach the API through it."], ["Review and create", "Step 4 of 4. Check what will be created. To work, an HTTP API needs at least one route, integration, stage and deployment."]] as const;

export function HttpWizard() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const where = useWhere();
  const refresh = useRefreshReads();
  const [w, setW] = useState<Wizard>(emptyWizard);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [working, setWorking] = useState(false);
  const [failure, setFailure] = useState<{ code: string; message: string; at: string; api: string | null } | null>(null);
  const rest = useRestApis().data?.item ?? [];
  const http = useHttpApis().data?.items ?? [];
  const fn = useResourceDetail("lambda", "functions", w.integration.fn ?? "", w.integration.kind === "lambda" && Boolean(w.integration.fn));
  const set = (patch: Partial<Wizard>) => { setW((x) => ({ ...x, ...patch })); setFailure(null); };
  const setInt = (patch: Partial<Wizard["integration"]>) => set({ integration: { ...w.integration, ...patch } });
  const path = w.path || defaultPath(w.integration);
  const problem = wizardProblem(step, w, []);
  const calls = wizardCalls({ ...w, path }, where);
  const go = (i: number) => { setStep(i); setFurthest((f) => Math.max(f, i)); setTried(false); };
  const next = () => { setTried(true); if (!problem) go(step + 1); };
  const fact = (label: string) => fn.data?.fields.find((x) => x.label === label)?.value;
  const create = async () => {
    setWorking(true);
    let at = "CreateApi";
    let api: string | null = null;
    try {
      const made = await callKindOp<{ apiId: string }>(HTTP, "CreateApi", calls[0]!.input);
      api = made.apiId;
      const ids = { api };
      at = "CreateIntegration";
      const integration = await callKindOp<{ integrationId: string }>(HTTP, "CreateIntegration", wizardCalls({ ...w, path }, where, ids)[1]!.input);
      const rest2 = wizardCalls({ ...w, path }, where, { api, integration: integration.integrationId });
      await runCalls(HTTP, rest2.slice(2), (o) => (at = o));
      refresh();
      void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: api }, search: { done: "created" } });
    } catch (e) {
      refresh();
      setFailure({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : String(e), at, api });
    } finally {
      setWorking(false);
    }
  };
  const [title, sub] = TITLES[step]!;
  const edit = (i: number) => <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => go(i)} data-testid={`edit-${STEPS[i]!.id}`}>Edit</Button>;
  const urls = invokeUrls({ id: "<api-id>", region: where.region, endpoint: where.endpoint, stage: w.stage, http: true });
  const row = (label: string, value: React.ReactNode) => <div className="contents"><dt className="border-b py-3 text-muted-foreground">{label}</dt><dd className="min-w-0 border-b py-3 break-words text-bright">{value}</dd></div>;
  return (
    <FormPage
      title={title}
      sub={sub}
      stepsAfterTitle
      crumbs={[...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: "Create API", to: "/resources/apigateway/restapis?do=create" }, { label: "Create HTTP API", to: "/resources/apigateway/restapis?do=create-http" }]}
      request={{ service: "apigatewayv2", ...calls[0]! }}
      requests={step === 3 ? calls.map((c) => ({ service: "apigatewayv2", operation: c.operation, input: c.input })) : undefined}
      showInput
      noCall={step !== 3}
      callTitle="The calls this will make"
      callNote={<>Each call also carries the new API&apos;s ID.</>}
      refusal={failure}
      banner={failure ? <Alert tone="bad" title={failure.api ? `${failure.at} was refused, after the API was made.` : "The API wasn't created."} testId="create-refused">The service answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”.{failure.api && <> The API <span className="font-mono">{failure.api}</span> exists without what came after; open it to finish.</>}</Alert> : undefined}
      submitLabel="Create"
      pending={working}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next, nextLabel: step === 0 ? "Next" : "Next", nextDisabled: tried && problem !== null }}
      disabledWhy={problem ?? undefined}
      disabled={false}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: "create" } })}
      onSubmit={() => void create()}
      testId="http-wizard"
    >
      {step === 0 && (
        <>
          <FormSection title="Integrations" description="An integration connects a route to a backend resource." testId="section-integrations">
            <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">Integration type</span><RadioCards<"lambda" | "http"> name="kind" label="Integration type" value={w.integration.kind} onChange={(kind) => setInt({ kind })} options={[{ value: "lambda", title: "Lambda", body: "A Lambda function answers." }, { value: "http", title: "HTTP URI", body: "An HTTP endpoint answers." }]} /></div>
            {w.integration.kind === "lambda" ? (
              <>
                <Field label="Lambda function" error={tried && !w.integration.fn ? "Choose the Lambda function." : undefined} hint={<>{where.region}. API Gateway needs permission to invoke the function, through a resource-based policy or an IAM role.{w.integration.fn && fact("Runtime") && <> <b className="font-mono font-semibold text-foreground">{w.integration.fn}</b> runs {fact("Runtime")}.</>}</>}>{(id) => <FunctionPicker id={id} by="name" value={w.integration.fn} onChange={(f) => setInt({ fn: f })} />}</Field>
                <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">Payload format version</span><Segmented<"2.0" | "1.0"> value={w.integration.version} onChange={(version) => setInt({ version })} label="Payload format version" testId="payload-version" options={[{ value: "2.0", label: "2.0" }, { value: "1.0", label: "1.0" }]} className="w-fit" /><p className="text-[13px] leading-5 text-muted-foreground">The shape of the event the function receives and of the response it must return. The console uses the latest version, 2.0, when you do not choose; 1.0 is the other option.</p></div>
              </>
            ) : (
              <>
                <Field label="Endpoint URL" error={tried ? (problem ?? undefined) : undefined} help={<>A <span className="font-mono">{"{param}"}</span> placeholder is filled from the request path.</>}>{(id) => <Input id={id} value={w.integration.url} onChange={(e) => setInt({ url: e.target.value })} placeholder="https://orders.larder.example.com/{proxy}" className="font-mono" autoComplete="off" spellCheck={false} data-testid="integration-url" />}</Field>
                <Field label="Method">{(id) => <Select value={w.integration.method} onValueChange={(method) => setInt({ method })}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="integration-method"><SelectValue /></SelectTrigger><SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent></Select>}</Field>
              </>
            )}
          </FormSection>
          <FormSection title="API details" testId="section-api">
            <Field label="API name" error={tried && w.name.trim() === "" ? "Enter an API name." : undefined} help={<><b>Required</b>, 1 to 128 characters.</>} hint={rest.length + http.length > 0 ? <>This Region already has {list([...rest.map((a) => `a REST API, ${a.name}`), ...http.map((a) => `an HTTP API, ${a.name}`)])}.</> : undefined}>{(id) => <Input id={id} value={w.name} onChange={(e) => set({ name: e.target.value })} placeholder="larder-reports" className="font-mono" autoComplete="off" spellCheck={false} data-testid="api-name" />}</Field>
            <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">IP address type</span><Segmented<"ipv4" | "dualstack"> value={w.ip} onChange={(ip) => set({ ip })} label="IP address type" testId="ip-type" options={[{ value: "ipv4", label: "IPv4" }, { value: "dualstack", label: "Dualstack" }]} className="w-fit" /><p className="text-[13px] leading-5 text-muted-foreground">Which addresses can invoke the API. <b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the setting and serves one local address (<span className="font-mono">crates/aws-apigateway/src/v2/apis.rs</span>).</p></div>
          </FormSection>
        </>
      )}
      {step === 1 && (
        <FormSection title="Routes" testId="section-routes">
          <div className="grid items-start gap-4 sm:grid-cols-[9rem_minmax(0,1fr)_minmax(0,14rem)]">
            <Field label="Method">{(id) => <Select value={w.method} onValueChange={(method) => set({ method })}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="route-method"><SelectValue /></SelectTrigger><SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent></Select>}</Field>
            <Field label="Path" error={tried ? (problem ?? undefined) : undefined}>{(id) => <Input id={id} value={path} onChange={(e) => set({ path: e.target.value })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="route-path" />}</Field>
            <Field label="Integration"><div className="flex h-9 items-center text-[14px] text-bright">{w.integration.kind === "lambda" ? `${w.integration.fn ?? "…"} Lambda` : "HTTP endpoint"}</div></Field>
          </div>
          <RuleHelp><b>How routes match.</b> The console names the first route after the Lambda function. <span className="font-mono">ANY</span> matches every method you have not defined for the path. A greedy variable such as <span className="font-mono">{"{proxy+}"}</span> catches all child paths, and must come last.</RuleHelp>
          <RuleHelp>A request goes to the most specific match: a full route and method, then a greedy path, then the <span className="font-mono">$default</span> route. The new API is separate from the others in {where.region}{http.length > 0 ? <>: {http[0]!.name} has its own routes</> : null}.</RuleHelp>
        </FormSection>
      )}
      {step === 2 && (
        <FormSection title="Stage" testId="section-stage">
          <Field label="Stage name" error={tried ? (problem ?? undefined) : undefined}>{(id) => <Input id={id} value={w.stage} onChange={(e) => set({ stage: e.target.value })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="stage-name" />}</Field>
          <div className="flex items-start gap-3"><Switch id="auto" checked={w.auto} onCheckedChange={(auto) => set({ auto })} className="mt-0.5" data-testid="stage-auto" /><div className="grid gap-0.5"><Label htmlFor="auto">Automatic deployments</Label><p className="text-[13px] text-muted-foreground">{w.auto ? "On" : "Off"}</p></div></div>
          <RuleHelp><b>What a stage does.</b> The <span className="font-mono">$default</span> stage is served from the base of the API&apos;s URL, with no stage name in the path. This API will answer at <span className="font-mono break-all">{urls.aws}{w.stage === "$default" ? path : `${path}`}</span>.</RuleHelp>
          <RuleHelp>With automatic deployment on, a change to a route, integration or authorizer is released for you. With it off, you publish with a deployment. Through the API itself, a new stage starts with it off.</RuleHelp>
        </FormSection>
      )}
      {step === 3 && (
        <FormSection title="Summary" testId="section-summary">
          <dl className="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-x-4 text-[14px] [&>div]:contents">
            <div className="col-span-3 mt-1 mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">API name</h3>{edit(0)}</div>
            {row("Name", <span className="font-mono">{w.name}</span>)}{row("Protocol", "HTTP")}{row("IP address type", w.ip === "ipv4" ? "IPv4" : "Dualstack")}
            <div className="col-span-3 mt-4 mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">Integration</h3>{edit(0)}</div>
            {row("Type", w.integration.kind === "lambda" ? "Lambda function" : "HTTP URI")}{w.integration.kind === "lambda" ? <>{row("Function", <span className="font-mono">{w.integration.fn}</span>)}{row("Payload format version", w.integration.version)}</> : row("Endpoint URL", <span className="font-mono break-all">{w.integration.url}</span>)}
            <div className="col-span-3 mt-4 mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">Route</h3>{edit(1)}</div>
            {row("Route", <span className="font-mono">{w.method} {path}</span>)}{row("Integration", w.integration.kind === "lambda" ? w.integration.fn : "HTTP endpoint")}
            <div className="col-span-3 mt-4 mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">Stage</h3>{edit(2)}</div>
            {row("Stage name", <span className="font-mono">{w.stage}</span>)}{row("Automatic deployments", w.auto ? "On" : "Off")}
          </dl>
        </FormSection>
      )}
    </FormPage>
  );
}
