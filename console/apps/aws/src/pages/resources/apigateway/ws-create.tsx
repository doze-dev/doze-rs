// Create WebSocket API: a name and the route selection expression that picks a route for each message. doze-aws serves WebSocket APIs
// (plane/ws): `$connect` runs first and may refuse, a message goes to the route its expression names or `$default`, and a backend answers
// through the @connections management API.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, useServiceCrumbs } from "../../../kit";
import { HTTP, useHttpApis, useWrite } from "./api";
import type { HttpApi } from "./types";
import { apiNameProblem } from "./words";

export function WsCreate() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const [name, setName] = useState("");
  const [expression, setExpression] = useState("$request.body.action");
  const [tried, setTried] = useState(false);
  const make = useWrite<HttpApi>(HTTP, "CreateApi");
  const taken = (useHttpApis().data?.items ?? []).filter((a) => a.protocolType === "WEBSOCKET").map((a) => a.name);
  const problem = apiNameProblem(name) ?? (expression.trim() === "" ? "Enter the route selection expression." : null);
  const input = useMemo(() => ({ name: name || "<name>", protocolType: "WEBSOCKET", routeSelectionExpression: expression }), [name, expression]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  return (
    <FormPage
      title="Create WebSocket API"
      sub="Persistent connections for two-way communication. A client connects at the API's stage address; each message goes to the route its expression selects."
      crumbs={[...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: "Create API", to: "/resources/apigateway/restapis?do=create" }, { label: "Create WebSocket API", to: "/resources/apigateway/restapis?do=create-websocket" }]}
      request={{ service: "apigatewayv2", operation: "CreateApi", input }}
      showInput
      refusal={failure}
      callNote={<>Add the routes (<span className="font-mono">$connect</span>, <span className="font-mono">$disconnect</span>, <span className="font-mono">$default</span> and your own), an integration for each and a stage on the API&apos;s page.</>}
      banner={failure ? <Alert tone="bad" title="The API wasn't created." testId="create-refused">CreateApi answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create API"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: "create" } })}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: (made) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: made.apiId }, search: { done: "created" } }) }); }}
      testId="ws-create"
    >
      <FormSection title="API details" testId="section-details">
        <Field label="API name" error={tried ? (apiNameProblem(name) ?? undefined) : undefined} hint={taken.length > 0 ? <>This Region already has a WebSocket API, {taken[0]}.</> : undefined}>{(id) => <Input id={id} value={name} onChange={(e) => { setName(e.target.value); make.reset(); }} placeholder="larder-live" className="font-mono" autoComplete="off" spellCheck={false} data-testid="api-name" />}</Field>
        <Field label="Route selection expression" error={tried && expression.trim() === "" ? "Enter the route selection expression." : undefined} help={<>Evaluated against each message, such as <span className="font-mono">$request.body.action</span>. Its value names a route; a message that selects none goes to <span className="font-mono">$default</span>. Set at creation.</>}>{(id) => <Input id={id} value={expression} onChange={(e) => setExpression(e.target.value)} className="font-mono" autoComplete="off" spellCheck={false} data-testid="api-expression" />}</Field>
      </FormSection>
    </FormPage>
  );
}
