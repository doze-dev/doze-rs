// Create HTTP API (apigateway__http-create, http-create-routes, http-create-stages, http-create-review): four steps (the integration and the
// API, the route, the stage, the review). Nothing is sent until the last step: CreateApi, CreateIntegration, CreateRoute, CreateStage.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { ApiFailure } from "@doze/ui/lib/api";
import { PlusIcon } from "lucide-react";
import { Field, FormPage, FormSection, RadioCards, RuleHelp, useServiceCrumbs } from "../../../kit";
import { HTTP, useHttpApis, useRefreshReads, useRestApis, useRoutes } from "./api";
import { type Backend, type Wizard, emptyBackend, emptyWizard, keyOfBackend, pathOf, wizardCalls, wizardProblem } from "./http-model";
import { IntegrationFields, RouteFields } from "./http-wizard-parts";
import { useWhere } from "./parts";
import { callKindOp } from "../../../kit";
import { haveApis, invokeUrls } from "./words";
import { sdkShown } from "./calls";

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
  const set = (patch: Partial<Wizard>) => { setW((x) => ({ ...x, ...patch })); setFailure(null); };
  const setBackend = (i: number, patch: Partial<Backend>) => set({ backends: w.backends.map((b, j) => (j === i ? { ...b, ...patch } : b)) });
  const problem = wizardProblem(step, w, []);
  const calls = wizardCalls(w, where);
  const go = (i: number) => { setStep(i); setFurthest((f) => Math.max(f, i)); setTried(false); };
  const next = () => { setTried(true); if (!problem) go(step + 1); };
  const create = async () => {
    setWorking(true);
    let at = "CreateApi";
    let api: string | null = null;
    try {
      // Each call is built when its turn comes, with the ids the ones before it returned.
      const made = { integrations: [] as string[] } as { api?: string; integrations: string[] };
      for (let n = 0; n < calls.length; n++) {
        const call = wizardCalls(w, where, made)[n]!;
        at = call.operation;
        const answer = await callKindOp<{ apiId?: string; integrationId?: string }>(HTTP, call.operation, call.input);
        if (call.operation === "CreateApi") { made.api = answer.apiId; api = answer.apiId ?? null; }
        else if (call.operation === "CreateIntegration" && answer.integrationId) made.integrations.push(answer.integrationId);
      }
      refresh();
      void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: api ?? "" }, search: { done: "created" } });
    } catch (e) {
      refresh();
      setFailure({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : String(e), at, api });
    } finally {
      setWorking(false);
    }
  };
  const [title, sub] = TITLES[step]!;
  const edit = (i: number, n: number) => <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => go(i)} data-testid={`edit-${STEPS[i]!.id}${n > 0 ? `-${n + 1}` : ""}`}>Edit</Button>;
  const urls = invokeUrls({ id: "<api-id>", region: where.region, endpoint: where.endpoint, stage: w.stage, http: true });
  const path = pathOf(w.backends[0]!);
  const other = [...http].sort((x, y) => x.name.localeCompare(y.name))[0];
  const otherRoutes = useRoutes(other?.apiId ?? "", other !== undefined).data?.items ?? [];
  // Review and create: the steps the person has not seen are checked first, and the first with a problem is where they land.
  const review = () => {
    for (let i = 0; i < 3; i++) if (wizardProblem(i, w, []) !== null) { setStep(i); setFurthest((f) => Math.max(f, i)); setTried(true); return; }
    go(3);
  };
  // `n`: which of several (the second integration is `summary-integration-2`); the first keeps the names it has always had.
  const section = (label: string, i: number, rows: [string, React.ReactNode][], n = 0) => (
    <div className="border-t pt-4 first:border-t-0 first:pt-0" data-testid={`summary-${STEPS[i]!.id}${n > 0 ? `-${n + 1}` : ""}`}>
      <div className="mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">{label}</h3>{edit(i, n)}</div>
      <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] text-[14px] [&>div:last-child>*]:border-b-0">
        {rows.map(([k, v]) => <div key={k} className="contents"><dt className="border-b py-3 text-muted-foreground">{k}</dt><dd className="min-w-0 border-b py-3 break-words text-bright">{v}</dd></div>)}
      </dl>
    </div>
  );
  const tableHead = (cols: string[], template: string) => <div className={`grid gap-4 rounded-lg bg-muted/60 px-4 py-3 text-[12px] tracking-[0.08em] text-muted-foreground uppercase ${template}`}>{cols.map((c) => <span key={c}>{c}</span>)}</div>;
  return (
    <FormPage
      shown={sdkShown}
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
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next }}
      hideFooter
      disabled={false}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: "create" } })}
      onSubmit={() => void create()}
      testId="http-wizard"
    >
      {step === 0 && (
        <>
          <FormSection title="Integrations" description="An integration connects a route to a backend resource." testId="section-integrations" action={<Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => set({ backends: [...w.backends, emptyBackend()] })} data-testid="add-integration"><PlusIcon /> Add integration</Button>}>
            {w.backends.map((b, i) => <IntegrationFields key={i} i={i} count={w.backends.length} form={b.integration} region={where.region} set={(patch) => setBackend(i, { integration: { ...b.integration, ...patch } })} remove={() => set({ backends: w.backends.filter((_, j) => j !== i) })} tried={tried} problem={problem} />)}
          </FormSection>
          <FormSection title="API details" testId="section-api">
            <Field label="API name" error={tried && w.name.trim() === "" ? "Enter an API name." : undefined} help={<><b>Required</b>, 1 to 128 characters.</>} hint={rest.length + http.length > 0 ? <>This Region already has {haveApis(rest.map((a) => a.name), http.map((a) => a.name))}.</> : undefined}>{(id) => <Input id={id} value={w.name} onChange={(e) => set({ name: e.target.value })} placeholder="larder-reports" className="font-mono" autoComplete="off" spellCheck={false} data-testid="api-name" />}</Field>
            <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">IP address type</span><RadioCards<"ipv4" | "dualstack"> name="ip" label="IP address type" value={w.ip} onChange={(ip) => set({ ip })} options={[{ value: "ipv4", title: "IPv4" }, { value: "dualstack", title: "Dualstack" }]} /><p className="text-[13px] leading-5 text-muted-foreground">Which addresses can invoke the API. <b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the setting and serves one local address.</p></div>
          </FormSection>
        </>
      )}
      {step === 1 && (
        <>
          <FormSection title="Routes" testId="section-routes">
            {tableHead(["Method", "Path", "Integration"], "grid-cols-[10rem_minmax(0,1fr)_minmax(0,16rem)]")}
            {w.backends.map((b, i) => <RouteFields key={i} i={i} b={b} set={(patch) => setBackend(i, patch)} tried={tried} problem={problem} />)}
          </FormSection>
          <FormSection title="How routes match" testId="section-match">
            <RuleHelp>The console names each route after its Lambda function. <b>ANY</b> matches every method you have not defined for the path. A greedy variable such as <span className="font-mono">{"{proxy+}"}</span> catches all child paths, and must come last.</RuleHelp>
            <RuleHelp>A request goes to the most specific match: a full route and method, then a greedy path, then the <span className="font-mono">$default</span> route.{other && otherRoutes.length > 0 ? <> {other.name} already has <span className="font-mono">{otherRoutes.slice(0, 2).map((r) => r.routeKey).join(" and ")}</span>; this new API is separate.</> : <> The new API is separate from any other in {where.region}.</>}</RuleHelp>
          </FormSection>
        </>
      )}
      {step === 2 && (
        <>
          <FormSection title="Stage" testId="section-stage">
            {tableHead(["Stage name", "Automatic deployments"], "grid-cols-[minmax(0,1fr)_minmax(0,1fr)]")}
            <div className="grid items-center gap-4 px-1 sm:grid-cols-2">
              <Field label={<span className="sr-only">Stage name</span>} error={tried ? (problem ?? undefined) : undefined}>{(id) => <Input id={id} value={w.stage} onChange={(e) => set({ stage: e.target.value })} className="max-w-64 font-mono" autoComplete="off" spellCheck={false} data-testid="stage-name" />}</Field>
              <div className="flex items-center gap-3"><Switch id="auto" checked={w.auto} onCheckedChange={(auto) => set({ auto })} data-testid="stage-auto" /><Label htmlFor="auto" className="font-normal">{w.auto ? "On" : "Off"}</Label></div>
            </div>
          </FormSection>
          <FormSection title="What a stage does" testId="section-stage-help">
            <RuleHelp>The <span className="font-mono">$default</span> stage is served from the base of the API&apos;s URL, with no stage name in the path. This API will answer at <span className="font-mono break-all">{urls.aws}{path}</span>.</RuleHelp>
            <RuleHelp>With automatic deployment on, a change to a route, integration or authorizer is released for you. With it off, you publish with a deployment. Through the API itself, a new stage starts with it off.</RuleHelp>
          </FormSection>
        </>
      )}
      {step === 3 && (
        <FormSection title="Summary" testId="section-summary">
          <div className="grid gap-4">
            {section("API name", 0, [["Name", <span key="n" className="font-mono">{w.name}</span>], ["Protocol", "HTTP"], ["IP address type", w.ip === "ipv4" ? "IPv4" : "Dualstack"]])}
            {w.backends.map((b, i) => section(w.backends.length > 1 ? `Integration ${i + 1}` : "Integration", 0, [["Type", b.integration.kind === "lambda" ? "Lambda function" : "HTTP URI"], ...(b.integration.kind === "lambda" ? ([["Function", <span key="f" className="font-mono">{b.integration.fn}</span>], ["Payload format version", b.integration.version]] as [string, React.ReactNode][]) : ([["Endpoint URL", <span key="u" className="font-mono break-all">{b.integration.url}</span>]] as [string, React.ReactNode][]))], i))}
            {w.backends.map((b, i) => section(w.backends.length > 1 ? `Route ${i + 1}` : "Route", 1, [["Route", <span key="r" className="font-mono">{keyOfBackend(b)}</span>], ["Integration", b.integration.kind === "lambda" ? b.integration.fn : "HTTP endpoint"]], i))}
            {section("Stage", 2, [["Stage name", <span key="s" className="font-mono">{w.stage}</span>], ["Automatic deployments", w.auto ? "On" : "Off"]])}
          </div>
        </FormSection>
      )}
      <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-2 border-t pt-5" data-slot="form-footer">
        {tried && problem && step < 3 && <p className="mr-auto text-[13px] text-muted-foreground" data-testid="disabled-why">{problem}</p>}
        <Button type="button" variant="outline" className="bg-card" onClick={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: "create" } })} data-testid="form-cancel">Cancel</Button>
        {step > 0 && <Button type="button" variant="outline" className="bg-card" onClick={() => go(step - 1)} data-testid="form-back">Previous</Button>}
        {step < 3 && <Button type="button" variant="outline" className="bg-card" onClick={review} data-testid="form-review">Review and create</Button>}
        <Button type="submit" disabled={working} data-testid="form-submit">{step < 3 ? "Next" : working ? "Working…" : "Create"}</Button>
      </div>
    </FormPage>
  );
}
