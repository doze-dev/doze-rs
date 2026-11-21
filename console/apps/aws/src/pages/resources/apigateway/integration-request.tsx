// A method's integration request page (apigateway__rest-integration-request): what the backend is sent (its type, verb, URL, timeout), the
// parameters mapped from the method request, and the mapping templates. What doze-aws applies and what it keeps is said where each is shown.
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { RuleHelp } from "../../../kit";
import { Card, Dim, Mono, Rows } from "./parts";
import type { RestMethod } from "./types";
import { integrationKind } from "./words";

type Mapped = { name: string; from: string; caching: boolean };
const mapped = (m: RestMethod, where: "querystring" | "path" | "header"): Mapped[] => {
  const prefix = `integration.request.${where}.`;
  const cached = new Set(m.methodIntegration?.cacheKeyParameters ?? []);
  return Object.entries(m.methodIntegration?.requestParameters ?? {}).filter(([k]) => k.startsWith(prefix)).map(([k, v]) => ({ name: k.slice(prefix.length), from: v, caching: cached.has(k) }));
};
const PASS: Record<string, string> = { WHEN_NO_MATCH: "When no template matches the content type, pass the body through", WHEN_NO_TEMPLATES: "Pass the body through only when the integration has no template", NEVER: "Never: a body with no matching template is refused (415)" };

export function IntegrationRequest({ method: m }: { method: RestMethod }) {
  const i = m.methodIntegration;
  if (!i) return <Card title="Integration request settings" testId="integration-settings"><p className="text-[14px] text-muted-foreground">This method has no integration yet, so nothing is sent anywhere.</p></Card>;
  const proxy = i.type === "AWS_PROXY" || i.type === "HTTP_PROXY";
  const lambda = integrationKind(i) === "Lambda function";
  const timeout = i.timeoutInMillis ?? 29000;
  const columns: DataColumn<Mapped>[] = [
    { id: "name", header: "Name", min: 200, accessorFn: (p) => p.name, cell: (c) => <span className="font-mono font-medium text-bright">{c.row.original.name}</span> },
    { id: "from", header: "Mapped from", size: 380, accessorFn: (p) => p.from, cell: (c) => <span className="font-mono text-[13px] break-all">{c.row.original.from}</span> },
    { id: "caching", header: "Caching", size: 160, accessorFn: (p) => (p.caching ? "Yes" : "No") },
  ];
  const queries = mapped(m, "querystring");
  const paths = mapped(m, "path");
  const headers = mapped(m, "header");
  const templates = Object.entries(i.requestTemplates ?? {});
  return (
    <>
      <Card title="Integration request settings" testId="integration-settings">
        <Rows rows={[
          { label: "Integration type", value: integrationKind(i) },
          ...(lambda || i.type === "HTTP" || i.type === "HTTP_PROXY" ? [{ label: lambda ? "Lambda proxy integration" : "HTTP proxy integration", value: proxy ? "On" : "Off" }] : []),
          ...(!lambda && i.type !== "MOCK" && i.httpMethod ? [{ label: "HTTP method", value: <Mono>{i.httpMethod}</Mono> }] : []),
          ...(i.type !== "MOCK" ? [{ label: lambda ? "Lambda function URI" : "Endpoint URL", value: i.uri ? <Mono>{i.uri}</Mono> : <Dim /> }] : []),
          ...(i.credentials ? [{ label: "Execution role", value: <Mono>{i.credentials}</Mono> }] : []),
          { label: "Content handling", value: i.contentHandling === "CONVERT_TO_BINARY" ? "Convert to binary" : i.contentHandling === "CONVERT_TO_TEXT" ? "Convert to text" : "Passthrough", note: "Stored, not applied locally: bodies are passed to the backend as they arrive." },
          { label: "Default timeout", value: timeout === 29000 ? "On (29 seconds)" : `Custom (${timeout.toLocaleString("en-US")} ms)` },
        ]} />
        <div className="mt-6 grid gap-3">
          <RuleHelp>{i.type === "HTTP" || i.type === "HTTP_PROXY" ? <><b>Endpoint URL</b> should be an HTTPS address; the developer guide recommends HTTPS for HTTP integrations. A <span className="font-mono">{"{param}"}</span> placeholder is filled from the request path. </> : null}<b>Default timeout</b> is 29 seconds; a custom timeout runs from 50 to 29,000 milliseconds. doze-aws applies it: a backend that is slower is answered with a 504.</RuleHelp>
        </div>
      </Card>
      <Card title="URL query string parameters" sub="Each parameter of the method request is mapped to a parameter the backend expects." testId="integration-queries">
          <DataTable<Mapped> aria-label="URL query string parameters" data={queries} columns={columns} rowId={(p) => p.name} empty="None are defined." />
          <RuleHelp className="mt-5">doze-aws applies these mappings when it calls the backend: a mapped parameter reaches it under the name on the left.</RuleHelp>
        </Card>
      <Card title="URL path parameters" sub={paths.length === 0 ? "None are defined." : undefined} testId="integration-paths">
        {paths.length > 0 && <DataTable<Mapped> aria-label="URL path parameters" data={paths} columns={columns} rowId={(p) => p.name} />}
      </Card>
      {headers.length > 0 && <Card title="HTTP request headers" testId="integration-headers"><DataTable<Mapped> aria-label="HTTP request headers" data={headers} columns={columns} rowId={(p) => p.name} /></Card>}
      <Card title="Mapping templates" sub={templates.length === 0 ? "No template is defined." : "A template (Velocity) turns the request into what the backend receives, picked by the caller's content type."} testId="integration-templates">
        {templates.length === 0 ? (
          <p className="text-[13.5px] text-muted-foreground">{PASS[i.passthroughBehavior ?? "WHEN_NO_MATCH"] ?? i.passthroughBehavior}. doze-aws runs Velocity templates (a subset; what it cannot run is refused by name).</p>
        ) : (
          <div className="grid gap-4">
            {templates.map(([type, text]) => (
              <div key={type} className="grid gap-1.5"><span className="font-mono text-[13px] font-medium text-bright" data-testid="template-type">{type}</span><CodeBlock text={text} testId="template-body" /></div>
            ))}
            <p className="text-[13px] text-muted-foreground">{PASS[i.passthroughBehavior ?? "WHEN_NO_MATCH"] ?? i.passthroughBehavior}.</p>
          </div>
        )}
      </Card>
    </>
  );
}
