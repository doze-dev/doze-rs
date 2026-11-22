// Edit integration request (apigateway__rest-integration-request, Edit): the timeout, the mapping templates and the parameters mapped from the
// method request, as UpdateIntegration's patches (`integration-model.ts`). A proxy integration has its timeout to edit and no more.
import { PlusIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field } from "../../../kit";
import { REST, useWrite } from "./api";
import { DEFAULT_TIMEOUT, type IntegrationEdit, PASSTHROUGH, type ParamRow, type Passthrough, type Where, editProblem, fromIntegration, integrationPatches, newId, shapes } from "./integration-model";
import type { Integration } from "./types";

const PASS: Record<Passthrough, string> = { WHEN_NO_MATCH: "When no template matches the content type", WHEN_NO_TEMPLATES: "Only when the integration has no template", NEVER: "Never: refuse it (415)" };

export function IntegrationEditDialog({ api, resource, method, integration, onClose }: { api: string; resource: string; method: string; integration: Integration; onClose: () => void }) {
  const save = useWrite(REST, "UpdateIntegration");
  const [e, setE] = useState<IntegrationEdit>(() => fromIntegration(integration));
  const [tried, setTried] = useState(false);
  const proxy = !shapes(integration);
  const problem = editProblem(e, proxy);
  const patches = integrationPatches(integration, e);
  const set = (p: Partial<IntegrationEdit>) => { setE((x) => ({ ...x, ...p })); save.reset(); };
  const failed = save.error ? errorWords(save.error) : null;
  const submit = () => {
    setTried(true);
    if (problem) return;
    if (patches.length === 0) return onClose();
    save.mutate({ restApiId: api, resourceId: resource, httpMethod: method, patchOperations: patches } as never, { onSuccess: onClose });
  };
  const setParam = (id: number, p: Partial<ParamRow>) => set({ params: e.params.map((x) => (x.id === id ? { ...x, ...p } : x)) });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-[720px]" data-testid="integration-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Edit integration request</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">What {method} sends to its backend: how long it waits{proxy ? "." : ", the templates that shape the body and the parameters mapped from the method request."}</DialogDescription></DialogHeader>
        <form className="grid gap-6" noValidate onSubmit={(ev) => { ev.preventDefault(); submit(); }}>
          <div className="grid gap-3">
            <div className="flex items-center gap-3"><Switch id="timeout-default" checked={e.defaultTimeout} onCheckedChange={(defaultTimeout) => set({ defaultTimeout })} data-testid="timeout-default" /><Label htmlFor="timeout-default">Default timeout ({DEFAULT_TIMEOUT / 1000} seconds)</Label></div>
            {!e.defaultTimeout && <Field label="Custom timeout" error={tried && problem?.startsWith("A custom timeout") ? problem : undefined} help="Milliseconds, from 50 to 29,000. A backend that is slower is answered with a 504.">{(id) => <div className="flex items-center gap-3"><Input id={id} value={e.timeout} onChange={(ev) => set({ timeout: ev.target.value })} inputMode="numeric" className="w-36 font-mono" data-testid="timeout-ms" /><span className="text-[13.5px] text-muted-foreground">ms</span></div>}</Field>}
          </div>
          {!proxy && (
            <>
              <div className="grid gap-3" data-testid="edit-templates">
                <h3 className="text-[15px] font-semibold text-bright">Mapping templates</h3>
                {e.templates.length === 0 && <p className="text-[13.5px] text-muted-foreground">No template is defined: the body is passed as it arrives, as the setting below says.</p>}
                {e.templates.map((t) => (
                  <div key={t.id} className="grid gap-2 rounded-[10px] border p-3" data-testid="template-row">
                    <div className="flex items-center gap-2"><Input value={t.type} onChange={(ev) => set({ templates: e.templates.map((x) => (x.id === t.id ? { ...x, type: ev.target.value } : x)) })} placeholder="application/json" aria-label="Content type" className="font-mono" autoComplete="off" spellCheck={false} data-testid="template-type-input" /><Button type="button" variant="ghost" size="icon" aria-label="Remove template" onClick={() => set({ templates: e.templates.filter((x) => x.id !== t.id) })}><XIcon /></Button></div>
                    <Textarea value={t.text} onChange={(ev) => set({ templates: e.templates.map((x) => (x.id === t.id ? { ...x, text: ev.target.value } : x)) })} rows={4} aria-label="Template" className="font-mono text-[12.5px]" spellCheck={false} data-testid="template-text-input" />
                  </div>
                ))}
                <div><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => set({ templates: [...e.templates, { id: newId(), type: "", text: "" }] })} data-testid="add-template"><PlusIcon /> Add template</Button></div>
                <Field label="When no template matches the request's content type">
                  {(id) => (
                    <Select value={e.passthrough} onValueChange={(v) => set({ passthrough: v as Passthrough })}>
                      <SelectTrigger id={id} className="w-full bg-card" data-testid="passthrough"><SelectValue /></SelectTrigger>
                      <SelectContent>{PASSTHROUGH.map((p) => <SelectItem key={p} value={p}>{PASS[p]}</SelectItem>)}</SelectContent>
                    </Select>
                  )}
                </Field>
              </div>
              <div className="grid gap-3" data-testid="edit-params">
                <h3 className="text-[15px] font-semibold text-bright">Parameter mappings</h3>
                {e.params.length === 0 && <p className="text-[13.5px] text-muted-foreground">No parameter is mapped.</p>}
                {e.params.map((p) => (
                  <div key={p.id} className="grid items-center gap-2 sm:grid-cols-[9rem_minmax(0,1fr)_minmax(0,1.4fr)_auto]" data-testid="param-row">
                    <Select value={p.where} onValueChange={(w) => setParam(p.id, { where: w as Where })}><SelectTrigger className="w-full bg-card" aria-label="Where" data-testid="param-where"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="querystring">Query string</SelectItem><SelectItem value="path">Path</SelectItem><SelectItem value="header">Header</SelectItem></SelectContent></Select>
                    <Input value={p.name} onChange={(ev) => setParam(p.id, { name: ev.target.value })} placeholder="name" aria-label="Name" className="font-mono" autoComplete="off" spellCheck={false} data-testid="param-name" />
                    <Input value={p.from} onChange={(ev) => setParam(p.id, { from: ev.target.value })} placeholder="method.request.querystring.page" aria-label="Mapped from" className="font-mono" autoComplete="off" spellCheck={false} data-testid="param-from" />
                    <Button type="button" variant="ghost" size="icon" aria-label="Remove parameter" onClick={() => set({ params: e.params.filter((x) => x.id !== p.id) })}><XIcon /></Button>
                  </div>
                ))}
                <div><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => set({ params: [...e.params, { id: newId(), where: "querystring", name: "", from: "" }] })} data-testid="add-param"><PlusIcon /> Add parameter</Button></div>
              </div>
            </>
          )}
          {tried && problem && !problem.startsWith("A custom timeout") && <p className="text-[13px] text-bad" role="alert" data-testid="integration-problem">{problem}</p>}
          {failed && <Alert tone="bad" title={`UpdateIntegration was refused${failed.code ? `: ${failed.code}` : ""}`} testId="integration-error">{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={save.isPending} data-testid="integration-save">{save.isPending ? "Saving…" : "Save changes"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
