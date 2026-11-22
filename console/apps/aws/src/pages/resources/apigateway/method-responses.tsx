// A method's response page (apigateway__rest-method-response): the status codes the method can return, each with its headers and its model
// by content type, Create response, and what doze-aws does with them.
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { PlainConfirm, RuleHelp } from "../../../kit";
import { REST, useWrite } from "./api";
import { Card, Dim, Mono, Rows } from "./parts";
import { CreateMethodResponse } from "./response-dialogs";
import type { RestMethod } from "./types";

export function MethodResponses({ api, resource, method: m }: { api: string; resource: string; method: RestMethod }) {
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const del = useWrite(REST, "DeleteMethodResponse");
  const all = Object.values(m.methodResponses ?? {}).sort((a, b) => a.statusCode.localeCompare(b.statusCode));
  return (
    <>
      <Card title="Method response settings" sub="One response per HTTP status code the method can return." action={<Button variant="outline" className="bg-card" onClick={() => setCreating(true)} data-testid="create-response"><PlusIcon /> Create response</Button>} testId="method-responses">
        {all.length === 0 ? <p className="text-[14px] text-muted-foreground" data-testid="no-responses">This method has no response. Create one for each status code it can return.</p> : (
          <div className="grid gap-8">
            {all.map((r) => {
              const headers = Object.keys(r.responseParameters ?? {}).map((k) => k.replace("method.response.header.", ""));
              const models = Object.entries(r.responseModels ?? {});
              return (
                <section key={r.statusCode} aria-label={`Response ${r.statusCode}`} data-testid="method-response">
                  <div className="mb-1 flex items-center justify-between gap-3"><h3 className="font-mono text-[15px] font-medium text-bright">{r.statusCode}</h3><Button variant="ghost" size="icon" aria-label={`Delete the ${r.statusCode} method response`} onClick={() => setDeleting(r.statusCode)} data-testid={`delete-${r.statusCode}`}><Trash2Icon /></Button></div>
                  <Rows rows={[
                    { label: "HTTP status code", value: <Mono>{r.statusCode}</Mono> },
                    { label: "Header name", value: headers.length > 0 ? <Mono>{headers.join(", ")}</Mono> : "None" },
                    { label: "Content type", value: models.length > 0 ? <Mono>{models.map(([t]) => t).join(", ")}</Mono> : <Dim>None</Dim> },
                    { label: "Model", value: models.length > 0 ? <Mono>{models.map(([, n]) => n).join(", ")}</Mono> : <Dim>None</Dim> },
                  ]} />
                </section>
              );
            })}
          </div>
        )}
      </Card>
      <Card title="What the caller gets if a response is missing" testId="method-response-help">
        <div className="grid gap-3">
          <RuleHelp>The console adds a 200 response to every method it creates. On AWS a non-proxy integration with no method response for its status answers 500 Internal server error even when the backend succeeded. doze-aws does not check method responses when it answers: the status and headers are the integration response&apos;s, so a missing method response is not a 500 here.</RuleHelp>
          <RuleHelp>You can&apos;t change the HTTP status code of a response; delete it and create a new one. A response&apos;s model name is stored with it and nothing reads it when requests are answered; <span className="font-mono">GetModels</span> lists the models of the API.</RuleHelp>
        </div>
      </Card>
      {creating && <CreateMethodResponse base={{ api, resource, method: m.httpMethod }} have={all.map((r) => r.statusCode)} onClose={() => setCreating(false)} />}
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete method response?" description={<>Delete the <b className="font-mono font-semibold text-bright">{deleting}</b> response of {m.httpMethod}?</>} call="DeleteMethodResponse" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate({ restApiId: api, resourceId: resource, httpMethod: m.httpMethod, statusCode: deleting } as never, { onSuccess: () => setDeleting(null) })} testId="method-response-delete" />
    </>
  );
}
