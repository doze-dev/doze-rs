// A method's integration response page (apigateway__rest-integration-response): each response with the method response it maps to, the regex
// that picks it and its mappings, and Create response. doze-aws applies the regex, the header mappings and the templates.
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { Button } from "@doze/ui/components/ui/button";
import { PlainConfirm, RuleHelp } from "../../../kit";
import { REST, useWrite } from "./api";
import { Card, Mono, Rows } from "./parts";
import { CreateIntegrationResponse } from "./response-dialogs";
import type { IntegrationResponse, RestMethod } from "./types";

export function IntegrationResponses({ api, resource, method: m }: { api: string; resource: string; method: RestMethod }) {
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const del = useWrite(REST, "DeleteIntegrationResponse");
  const i = m.methodIntegration;
  const all: IntegrationResponse[] = Object.values(i?.integrationResponses ?? {}).sort((a, b) => (a.selectionPattern ? 1 : 0) - (b.selectionPattern ? 1 : 0) || a.statusCode.localeCompare(b.statusCode));
  const base = { api, resource, method: m.httpMethod };
  const proxy = i?.type === "AWS_PROXY" || i?.type === "HTTP_PROXY";
  return (
    <>
      <Card title="Integration responses" sub="Each response maps a backend result to a method response status code." action={<Button variant="outline" className="bg-card" disabled={!i} onClick={() => setCreating(true)} data-testid="create-response"><PlusIcon /> Create response</Button>} testId="integration-responses">
        {!i ? <p className="text-[14px] text-muted-foreground">This method has no integration yet.</p> : all.length === 0 ? <p className="text-[14px] text-muted-foreground" data-testid="no-responses">{proxy ? "A proxy integration has no integration response to set: the backend's answer is returned as it is." : "No integration response is set. A non-proxy integration needs at least one, and the default one has an empty regex."}</p> : (
          <div className="grid gap-8">
            {all.map((r) => (
              <section key={r.statusCode} aria-label={`Response ${r.statusCode}`} data-testid="integration-response">
                <div className="mb-1 flex items-center justify-between gap-3"><h3 className="text-[15px] font-medium text-bright">{r.selectionPattern ? <>Response for <span className="font-mono">{r.selectionPattern}</span></> : "Default - Response"}</h3><Button variant="ghost" size="icon" aria-label={`Delete the ${r.statusCode} integration response`} onClick={() => setDeleting(r.statusCode)} data-testid={`delete-${r.statusCode}`}><Trash2Icon /></Button></div>
                <Rows rows={[
                  { label: "Method response status", value: <Mono>{r.statusCode}</Mono> },
                  { label: "HTTP status regex", value: r.selectionPattern ? <Mono>{r.selectionPattern}</Mono> : "Empty: this is the default response" },
                  { label: "Content handling", value: r.contentHandling === "CONVERT_TO_BINARY" ? "Convert to binary" : r.contentHandling === "CONVERT_TO_TEXT" ? "Convert to text" : "Passthrough", note: "Stored, not applied locally." },
                ]} />
              </section>
            ))}
          </div>
        )}
      </Card>
      {i && (
        <Card title="Mapping templates" sub={all.length === 0 ? "No response is set." : `Header mappings and body templates of ${all.length === 1 ? `the ${all[0]!.statusCode} response` : "each response"}.`} testId="integration-mappings">
          {all.map((r) => {
            const headers = Object.entries(r.responseParameters ?? {});
            const templates = Object.entries(r.responseTemplates ?? {});
            return (
              <div key={r.statusCode} className="mb-5 grid gap-3 last:mb-0" data-testid="response-mapping">
                <span className="text-[13px] font-medium text-bright">{r.statusCode}</span>
                {headers.length === 0 && templates.length === 0 ? <p className="text-[13.5px] text-muted-foreground">No header mappings or templates are defined for the {r.statusCode} response: the backend&apos;s answer is returned as it is.</p> : null}
                {headers.length > 0 && <Rows rows={headers.map(([k, v]) => ({ label: k.replace("method.response.header.", ""), value: <Mono>{v}</Mono> }))} testId="header-mappings" />}
                {templates.map(([t, body]) => <div key={t} className="grid gap-1.5"><span className="font-mono text-[13px] text-bright">{t}</span><CodeBlock text={body} /></div>)}
              </div>
            );
          })}
          <RuleHelp className="mt-4">For a non-proxy integration, set at least one integration response and make it the default. A response with an empty regex is the default response: it catches every result no other response matches. A proxy integration, such as a Lambda proxy, has no integration response to set.</RuleHelp>
          <RuleHelp className="mt-3">doze-aws applies the regex, the header mappings and the body template when it answers; content handling is stored and not applied.</RuleHelp>
        </Card>
      )}
      {creating && <CreateIntegrationResponse base={base} have={all.map((r) => r.statusCode)} statuses={Object.keys(m.methodResponses ?? {})} onClose={() => setCreating(false)} />}
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete integration response?" description={<>Delete the <b className="font-mono font-semibold text-bright">{deleting}</b> integration response of {m.httpMethod}? A backend result it matched then matches the default response, or none.</>} call="DeleteIntegrationResponse" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate({ restApiId: api, resourceId: resource, httpMethod: m.httpMethod, statusCode: deleting } as never, { onSuccess: () => setDeleting(null) })} testId="integration-response-delete" />
    </>
  );
}
