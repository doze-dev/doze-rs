// A method's request page (apigateway__rest-method-request): its settings (authorization, API key, operation name, method ARN), the query
// string parameters it accepts and the headers it declares. Edit changes the settings with UpdateMethod.
import { PencilIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Field, RuleHelp } from "../../../kit";
import { REST, type Patch, useRestPlans, useWrite } from "./api";
import { Card, Dim, Mono, Rows, useWhere } from "./parts";
import type { Authorizer, RestMethod, RestResource } from "./types";
import { authorizationWords, list, methodArn } from "./words";

type Param = { name: string; required: boolean; caching: boolean };
export const paramsOf = (m: RestMethod, where: "querystring" | "header"): Param[] => {
  const cached = new Set(m.methodIntegration?.cacheKeyParameters ?? []);
  return Object.entries(m.requestParameters ?? {}).filter(([k]) => k.startsWith(`method.request.${where}.`)).map(([k, v]) => ({ name: k.slice(`method.request.${where}.`.length), required: v, caching: cached.has(k) }));
};

const yes = (v: boolean) => (v ? "Yes" : "No");

export function MethodRequest({ api, apiName, method: m, resource, authorizers }: { api: string; apiName: string; method: RestMethod; resource: RestResource; authorizers: Authorizer[] }) {
  const { region, account } = useWhere();
  const plans = useRestPlans().data?.item ?? [];
  const [editing, setEditing] = useState(false);
  const covering = plans.filter((p) => p.apiStages?.some((s) => s.apiId === api));
  const authorizer = authorizers.find((a) => a.id === m.authorizerId);
  const columns: DataColumn<Param>[] = [
    { id: "name", header: "Name", min: 200, accessorFn: (p) => p.name, cell: (c) => <span className="font-mono font-medium text-bright">{c.row.original.name}</span> },
    { id: "required", header: "Required", size: 240, accessorFn: (p) => yes(p.required) },
    { id: "caching", header: "Caching", size: 240, accessorFn: (p) => yes(p.caching) },
  ];
  const queries = paramsOf(m, "querystring");
  const headers = paramsOf(m, "header");
  return (
    <>
      <Card title="Method request settings" action={<Button variant="outline" size="sm" className="bg-card" onClick={() => setEditing(true)} data-testid="edit-settings"><PencilIcon /> Edit</Button>} testId="method-settings">
        <Rows rows={[
          { label: "Authorization", value: authorizer ? <>{authorizationWords(m.authorizationType)}: <span className="font-mono">{authorizer.name}</span></> : authorizationWords(m.authorizationType) },
          { label: "API key required", value: m.apiKeyRequired ? "True" : "False" },
          { label: "Operation name", value: m.operationName ? <Mono>{m.operationName}</Mono> : <Dim /> },
          { label: "Method ARN", value: <Mono>{methodArn(region, account, api, m.httpMethod, resource.path)}</Mono> },
        ]} />
        <div className="mt-6 grid gap-3">
          <RuleHelp><b>Authorization</b> None gives open access to the method; to use a Lambda authorizer, pick a token or request authorizer from the list. {authorizers.length === 0 ? <>{apiName} has no authorizer yet.</> : <>{apiName} has {authorizers.length === 1 ? "one" : authorizers.length}, <span className="font-mono">{list(authorizers.map((a) => a.name))}</span>; {authorizer ? <>this method uses <span className="font-mono">{authorizer.name}</span>.</> : "this method does not use it."}</>}</RuleHelp>
          <RuleHelp><b>API key required</b> serves the method only to a request whose <span className="font-mono">x-api-key</span> exists, is enabled and is attached to a usage plan covering the stage; anything else gets 403 Forbidden. {covering.length > 0 ? <><span className="font-mono">{list(covering.map((p) => p.name))}</span> {covering.length === 1 ? "covers" : "cover"} {apiName}, stage {list(covering.flatMap((p) => (p.apiStages ?? []).filter((s) => s.apiId === api).map((s) => s.stage)))}.</> : <>No usage plan covers a stage of {apiName} yet.</>}</RuleHelp>
        </div>
      </Card>
      <Card title="URL query string parameters" sub="Parameters the caller may send. The integration request maps them to the backend." testId="method-queries">
        <DataTable<Param> aria-label="URL query string parameters" data={queries} columns={columns} rowId={(p) => p.name} empty="No query string parameter is declared on this method." />
      </Card>
      <Card title="HTTP request headers" sub={headers.length === 0 ? "No headers are declared on this method." : "Headers the caller may send."} testId="method-headers">
        {headers.length > 0 && <DataTable<Param> aria-label="HTTP request headers" data={headers} columns={columns} rowId={(p) => p.name} />}
      </Card>
      {editing && <SettingsEdit api={api} resource={resource.id} method={m} authorizers={authorizers} onClose={() => setEditing(false)} />}
    </>
  );
}

function SettingsEdit({ api, resource, method: m, authorizers, onClose }: { api: string; resource: string; method: RestMethod; authorizers: Authorizer[]; onClose: () => void }) {
  const save = useWrite(REST, "UpdateMethod");
  const [auth, setAuth] = useState(m.authorizationType === "COGNITO_USER_POOLS" ? "CUSTOM" : (m.authorizationType ?? "NONE"));
  const [authorizer, setAuthorizer] = useState(m.authorizerId ?? authorizers[0]?.id ?? "");
  const [key, setKey] = useState(Boolean(m.apiKeyRequired));
  const [name, setName] = useState(m.operationName ?? "");
  const patches: Patch[] = [];
  if (auth !== (m.authorizationType ?? "NONE") && !(auth === "CUSTOM" && m.authorizationType === "COGNITO_USER_POOLS")) patches.push({ op: "replace", path: "/authorizationType", value: auth });
  if (auth === "CUSTOM" && authorizer !== (m.authorizerId ?? "")) patches.push({ op: "replace", path: "/authorizerId", value: authorizer });
  if (key !== Boolean(m.apiKeyRequired)) patches.push({ op: "replace", path: "/apiKeyRequired", value: String(key) });
  if (name !== (m.operationName ?? "")) patches.push({ op: "replace", path: "/operationName", value: name });
  const failed = save.error ? errorWords(save.error) : null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]" data-testid="settings-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Edit method request</DialogTitle><DialogDescription className="text-[14px] leading-[22px]">{m.httpMethod} settings: who may call, and whether an API key is needed.</DialogDescription></DialogHeader>
        <form className="grid gap-5" onSubmit={(e) => { e.preventDefault(); if (patches.length === 0) return onClose(); save.mutate({ restApiId: api, resourceId: resource, httpMethod: m.httpMethod, patchOperations: patches } as never, { onSuccess: onClose }); }}>
          <Field label="Authorization">
            {(id) => (
              <Select value={auth} onValueChange={setAuth}>
                <SelectTrigger id={id} className="w-full bg-card" data-testid="settings-authorization"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="NONE">None</SelectItem><SelectItem value="AWS_IAM">IAM</SelectItem>{authorizers.length > 0 && <SelectItem value="CUSTOM">Authorizer</SelectItem>}</SelectContent>
              </Select>
            )}
          </Field>
          {auth === "CUSTOM" && (
            <Field label="Authorizer">
              {(id) => (
                <Select value={authorizer} onValueChange={setAuthorizer}>
                  <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="settings-authorizer"><SelectValue /></SelectTrigger>
                  <SelectContent>{authorizers.map((a) => <SelectItem key={a.id} value={a.id} className="font-mono">{a.name}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          )}
          <div className="flex items-center gap-3"><Switch id="settings-key" checked={key} onCheckedChange={setKey} data-testid="settings-key" /><Label htmlFor="settings-key">API key required</Label></div>
          <Field label="Operation name" optional>{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} className="font-mono" autoComplete="off" data-testid="settings-operation" />}</Field>
          {failed && <Alert tone="bad" title={`UpdateMethod was refused${failed.code ? `: ${failed.code}` : ""}`} testId="settings-error">{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={save.isPending} data-testid="settings-save">{save.isPending ? "Saving…" : "Save changes"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
