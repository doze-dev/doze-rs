// Create integration (apigateway__http-route-integration, Manage integrations, Create): a Lambda function or an HTTP endpoint, and the route
// to attach it to (UpdateRoute), if one is chosen.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, FunctionPicker, RadioCards, Segmented } from "../../../kit";
import { HTTP, runCalls, useRefreshReads, useRoutes, useWrite } from "./api";
import { type IntegrationForm, emptyIntegration, integrationInput, integrationProblem, ROUTE_METHODS } from "./http-model";
import { useWhere } from "./parts";
import type { HttpIntegration } from "./types";

const NONE = "__none__";

export function IntegrationCreate({ api, apiName, route, crumbs, onClose }: { api: string; apiName: string; route: string | undefined; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const where = useWhere();
  const refresh = useRefreshReads();
  const routes = useRoutes(api).data?.items ?? [];
  const [f, setF] = useState<IntegrationForm>(emptyIntegration);
  const [attach, setAttach] = useState(route ?? NONE);
  const [tried, setTried] = useState(false);
  const [late, setLate] = useState<string | null>(null);
  const make = useWrite<HttpIntegration>(HTTP, "CreateIntegration");
  const problem = integrationProblem(f);
  const input = useMemo(() => integrationInput(api, f, where), [api, f, where]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const target = routes.find((r) => r.routeId === attach);
  const submit = () => {
    setTried(true);
    if (problem) return;
    make.mutate(input as never, {
      onSuccess: async (made) => {
        if (target) {
          try { await runCalls(HTTP, [{ operation: "UpdateRoute", input: { ApiId: api, RouteId: target.routeId, target: `integrations/${made.integrationId}` } }]); } catch (e) { refresh(); setLate(e instanceof Error ? e.message : String(e)); return; }
        }
        refresh();
        onClose(made.integrationId);
      },
    });
  };
  const set = (patch: Partial<IntegrationForm>) => { setF((x) => ({ ...x, ...patch })); make.reset(); setLate(null); };
  return (
    <FormPage
      title="Create integration"
      sub="An integration connects a route to a backend such as a Lambda function or an HTTP endpoint."
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Integrations", to: `/resources/apigateway/apis/${api}?tab=integrations` }, { label: "Create integration", to: `/resources/apigateway/apis/${api}?do=create-integration` }]}
      request={{ service: "apigatewayv2", operation: "CreateIntegration", input }}
      requests={[{ service: "apigatewayv2", operation: "CreateIntegration", input }, ...(target ? [{ service: "apigatewayv2", operation: "UpdateRoute", input: { ApiId: api, RouteId: target.routeId, target: "integrations/<integration ID>" } }] : [])]}
      showInput
      refusal={failure}
      callNote={target ? <>The route is attached in a second call, with the new integration&apos;s ID.</> : undefined}
      banner={failure || late ? <Alert tone="bad" title={late ? "The integration was made, but the route was not attached." : "The integration wasn't created."} testId="create-refused">{failure ? <>CreateIntegration answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</> : late}</Alert> : undefined}
      submitLabel="Create integration"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={submit}
      testId="integration-create"
    >
      <FormSection title="Integration details" testId="section-details">
        <RadioCards<"lambda" | "http"> name="kind" label="Integration type" value={f.kind} onChange={(kind) => set({ kind })} options={[{ value: "lambda", title: "Lambda function", body: "Run a function for each request." }, { value: "http", title: "HTTP URI", body: "Forward each request to an HTTP endpoint." }]} />
        {f.kind === "lambda" ? (
          <>
            <Field label="Lambda function" error={tried && !f.fn ? "Choose the Lambda function." : undefined} hint={<>The function&apos;s name, in {where.region}.</>}>{(id) => <FunctionPicker id={id} by="name" value={f.fn} onChange={(fn) => set({ fn })} />}</Field>
            <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">Payload format version</span><Segmented<"2.0" | "1.0"> value={f.version} onChange={(version) => set({ version })} label="Payload format version" testId="payload-version" options={[{ value: "2.0", label: "2.0" }, { value: "1.0", label: "1.0" }]} className="w-fit" /><p className="text-[13px] leading-5 text-muted-foreground">The shape of the event the function receives and of the response it must return. 2.0 is the latest.</p></div>
          </>
        ) : (
          <>
            <Field label="Endpoint URL" error={tried ? (problem ?? undefined) : undefined} help={<>A <span className="font-mono">{"{param}"}</span> placeholder is filled from the request path; <span className="font-mono">{"{proxy}"}</span> is the rest of the path.</>}>{(id) => <Input id={id} value={f.url} onChange={(e) => set({ url: e.target.value })} placeholder="https://orders.larder.example.com/{proxy}" className="font-mono" autoComplete="off" spellCheck={false} data-testid="integration-url" />}</Field>
            <Field label="Method">
              {(id) => (
                <Select value={f.method} onValueChange={(method) => set({ method })}>
                  <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="integration-method"><SelectValue /></SelectTrigger>
                  <SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          </>
        )}
        <Field label="Attach this integration to a route" optional>
          {(id) => (
            <Select value={attach} onValueChange={setAttach}>
              <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="integration-route"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value={NONE} className="font-sans">Do not attach it yet</SelectItem>{routes.map((r) => <SelectItem key={r.routeId} value={r.routeId} className="font-mono">{r.routeKey}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
      </FormSection>
    </FormPage>
  );
}
