// Create response dialogs of a method: its method response (the contract with the caller) and its integration response (how a backend's
// answer becomes one). Each shows the call it makes.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, newRow, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { CallPanel, Field, RuleHelp } from "../../../kit";
import { REST, useWrite } from "./api";
import { type IntegrationResponseForm, type MethodResponseForm, integrationResponseInput, integrationResponseProblem, methodResponseInput, methodResponseProblem } from "./response-model";
import { useKindOpRead } from "../../../kit";

type Base = { api: string; resource: string; method: string };

function Frame({ title, description, children, onClose, call, problem, pending, onSubmit, failed, testId }: { title: string; description: string; children: React.ReactNode; onClose: () => void; call: { operation: string; input: object }; problem: string | null; pending: boolean; onSubmit: () => void; failed: { code: string; message: string } | null; testId: string }) {
  const [tried, setTried] = useState(false);
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-[640px]" data-testid={testId}>
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">{description}</DialogDescription></DialogHeader>
        <form className="grid gap-5" noValidate onSubmit={(e) => { e.preventDefault(); setTried(true); if (!problem) onSubmit(); }}>
          {children}
          {tried && problem && <p className="text-[13px] text-bad" role="alert" data-testid={`${testId}-problem`}>{problem}</p>}
          {failed && <Alert tone="bad" title={`${call.operation} was refused: ${failed.code}`} testId={`${testId}-error`}>{failed.message}</Alert>}
          <CallPanel request={{ service: "apigateway", operation: call.operation, input: call.input }} showInput testId={`${testId}-call`} />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={pending} data-testid={`${testId}-submit`}>{pending ? "Creating…" : "Create response"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function CreateMethodResponse({ base, have, onClose }: { base: Base; have: string[]; onClose: () => void }) {
  const [f, setF] = useState<MethodResponseForm>({ status: "", headers: "", contentType: "application/json", model: "Empty" });
  const models = useKindOpRead<{ item?: { name: string }[] }>(REST, "GetModels", { restApiId: base.api }).data?.item?.map((m) => m.name) ?? [];
  const make = useWrite(REST, "PutMethodResponse");
  const input = methodResponseInput(base, f);
  const failed = make.error ? errorWords(make.error) : null;
  return (
    <Frame title="Create method response" description="One response per HTTP status code the method can return." onClose={onClose} call={{ operation: "PutMethodResponse", input }} problem={methodResponseProblem(f, have)} pending={make.isPending} onSubmit={() => make.mutate(input as never, { onSuccess: onClose })} failed={failed?.code ? { code: failed.code, message: failed.message } : failed ? { code: "Error", message: failed.message } : null} testId="method-response-dialog">
      <Field label="HTTP status code">{(id) => <Input id={id} value={f.status} onChange={(e) => setF({ ...f, status: e.target.value })} placeholder="404" inputMode="numeric" className="font-mono" autoComplete="off" data-testid="response-status" />}</Field>
      <Field label="Header names" optional help="Comma-separated, such as X-Request-Id, X-Trace.">{(id) => <Input id={id} value={f.headers} onChange={(e) => setF({ ...f, headers: e.target.value })} className="font-mono" autoComplete="off" data-testid="response-headers" />}</Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Content type" optional>{(id) => <Input id={id} value={f.contentType} onChange={(e) => setF({ ...f, contentType: e.target.value })} className="font-mono" autoComplete="off" data-testid="response-content-type" />}</Field>
        <Field label="Model">
          {(id) => (
            <Select value={f.model} onValueChange={(model) => setF({ ...f, model })}>
              <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="response-model"><SelectValue /></SelectTrigger>
              <SelectContent>{["Empty", "Error", ...models.filter((m) => m !== "Empty" && m !== "Error")].map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
      </div>
      <RuleHelp>You can&apos;t change the HTTP status code of a response; delete it and create a new one.</RuleHelp>
    </Frame>
  );
}

export function CreateIntegrationResponse({ base, have, statuses, onClose }: { base: Base; have: string[]; statuses: string[]; onClose: () => void }) {
  const free = statuses.filter((st) => !have.includes(st));
  const [f, setF] = useState<IntegrationResponseForm>({ status: free[0] ?? "", pattern: "", headers: [], template: "", contentType: "application/json" });
  const [rows, setRows] = useState<KeyValueRow[]>([newRow()]);
  const make = useWrite(REST, "PutIntegrationResponse");
  const form = { ...f, headers: filled(rows).map((r) => ({ name: r.key, value: r.value })) };
  const input = integrationResponseInput(base, form);
  const failed = make.error ? errorWords(make.error) : null;
  return (
    <Frame title="Create integration response" description="Each response maps a backend result to a method response status code." onClose={onClose} call={{ operation: "PutIntegrationResponse", input }} problem={integrationResponseProblem(form, have)} pending={make.isPending} onSubmit={() => make.mutate(input as never, { onSuccess: onClose })} failed={failed ? { code: failed.code || "Error", message: failed.message } : null} testId="integration-response-dialog">
      <Field label="Method response status" hint={free.length === 0 ? "Every method response of this method has an integration response already. Create a method response for another status first." : "An integration response maps to a method response of the method: create the method response first."}>
        {(id) => (
          <Select value={f.status} onValueChange={(status) => setF({ ...f, status })} disabled={free.length === 0}>
            <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="response-status"><SelectValue placeholder="No method response is free" /></SelectTrigger>
            <SelectContent>{free.map((st) => <SelectItem key={st} value={st} className="font-mono">{st}</SelectItem>)}</SelectContent>
          </Select>
        )}
      </Field>
      <Field label="HTTP status regex" optional help="Matched against the backend's status code (an HTTP or mock backend) or error message (a Lambda function). Empty: this is the default response.">{(id) => <Input id={id} value={f.pattern} onChange={(e) => setF({ ...f, pattern: e.target.value })} placeholder="4\d\d" className="font-mono" autoComplete="off" data-testid="response-pattern" />}</Field>
      <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">Header mappings <span className="font-normal text-muted-foreground">optional</span></span><KeyValueEditor rows={rows} onChange={setRows} keyLabel="Header name" valueLabel="Mapped from" addLabel="Add header" removeLabel="Remove" testId="response-header-rows" /><RuleHelp>A value is a constant in single quotes, or <span className="font-mono">integration.response.header.X</span>.</RuleHelp></div>
      <Field label="Body mapping template" optional help="A Velocity template for the response body, for the content type below.">{(id) => <Textarea id={id} value={f.template} onChange={(e) => setF({ ...f, template: e.target.value })} rows={3} className="font-mono text-[13px]" data-testid="response-template" />}</Field>
    </Frame>
  );
}
