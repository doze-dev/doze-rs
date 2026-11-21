// Create route (apigateway__http-routes-none, Create route): an HTTP method and a path (a WebSocket API's route is a bare key), and the
// integration it goes to, if one exists. The route's authorizer is attached on the Authorization page.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection } from "../../../kit";
import { HTTP, useIntegrations, useRoutes, useWrite } from "./api";
import { ROUTE_METHODS, routeInput, routeKeyOf, routeProblem } from "./http-model";
import type { Route } from "./types";
import { httpTarget, lambdaName } from "./words";

const NONE = "__none__";

export function RouteCreate({ api, apiName, websocket, crumbs, onClose }: { api: string; apiName: string; websocket: boolean; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const taken = (useRoutes(api).data?.items ?? []).map((r) => r.routeKey);
  const integrations = useIntegrations(api).data?.items ?? [];
  const [method, setMethod] = useState("ANY");
  const [path, setPath] = useState("");
  const [integration, setIntegration] = useState(NONE);
  const [tried, setTried] = useState(false);
  const make = useWrite<Route>(HTTP, "CreateRoute");
  const key = websocket ? path.trim() : routeKeyOf(method, path);
  const problem = routeProblem(method, path, taken, websocket);
  const input = useMemo(() => routeInput(api, key || "<route key>", integration === NONE ? null : integration), [api, key, integration]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  return (
    <FormPage
      title="Create route"
      sub="A route maps a request method and path to an integration."
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Routes", to: `/resources/apigateway/apis/${api}` }, { label: "Create route", to: `/resources/apigateway/apis/${api}?do=create-route` }]}
      request={{ service: "apigatewayv2", operation: "CreateRoute", input }}
      showInput
      refusal={failure}
      banner={failure ? <Alert tone="bad" title="The route wasn't created." testId="create-refused">CreateRoute answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create route"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: () => onClose(key) }); }}
      testId="route-create"
    >
      <FormSection title="Route details" testId="section-details">
        {websocket ? (
          <Field label="Route key" error={tried ? (problem ?? undefined) : undefined} help={<>A key the route selection expression can produce, such as <span className="font-mono">sendMessage</span>, or <span className="font-mono">$connect</span>, <span className="font-mono">$disconnect</span> and <span className="font-mono">$default</span>.</>}>
            {(id) => <Input id={id} value={path} onChange={(e) => { setPath(e.target.value); make.reset(); }} placeholder="sendMessage" className="font-mono" autoComplete="off" spellCheck={false} data-testid="route-path" />}
          </Field>
        ) : (
          <div className="grid gap-4 sm:grid-cols-[10rem_minmax(0,1fr)]">
            <Field label="Method">
              {(id) => (
                <Select value={method} onValueChange={(m) => { setMethod(m); make.reset(); }}>
                  <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="route-method"><SelectValue /></SelectTrigger>
                  <SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
            <Field label="Path" error={tried ? (problem ?? undefined) : undefined} help={<>A path such as <span className="font-mono">/reports</span>; <span className="font-mono">{"{id}"}</span> matches one segment, <span className="font-mono">{"{proxy+}"}</span> catches every child path and must come last.</>}>
              {(id) => <Input id={id} value={path} onChange={(e) => { setPath(e.target.value); make.reset(); }} placeholder="/reports" className="font-mono" autoComplete="off" spellCheck={false} data-testid="route-path" />}
            </Field>
          </div>
        )}
        <Field label="Integration" optional hint={integrations.length === 0 ? <>{apiName} has no integration yet. Create the route now and attach one on the Integrations page.</> : undefined}>
          {(id) => (
            <Select value={integration} onValueChange={setIntegration}>
              <SelectTrigger id={id} className="w-full bg-card" data-testid="route-integration"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>No integration yet</SelectItem>
                {integrations.map((i) => <SelectItem key={i.integrationId} value={i.integrationId}><span className="font-mono">{i.integrationId}</span> <span className="text-muted-foreground">{lambdaName(i.integrationUri) ?? httpTarget(i.integrationType, i.integrationUri)}</span></SelectItem>)}
              </SelectContent>
            </Select>
          )}
        </Field>
      </FormSection>
    </FormPage>
  );
}
