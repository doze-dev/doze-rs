// Create method (apigateway__rest-method-create): the verb, what answers it (a Lambda function, an HTTP endpoint, a mock, an AWS service),
// and the method request's settings. Two calls (PutMethod, PutIntegration), and for a non-proxy integration the 200 responses.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, FunctionPicker, RadioCards, RolePicker, RuleHelp } from "../../../kit";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { REST, runCalls, useFunctionSentence, useAuthorizers, useRefreshReads, useResources } from "./api";
import { sdkShown } from "./calls";
import { type IntegrationKind, type MethodForm, METHODS, SERVICES, emptyMethod, methodCalls, methodProblem, settingsSummary } from "./method-model";
import { useWhere } from "./parts";
import { list, methodsOf } from "./words";

export function MethodCreate({ api, apiName, resource, crumbs, onClose }: { api: string; apiName: string; resource: string | undefined; crumbs: { label: string; to: string }[]; onClose: (created?: string, resource?: string) => void }) {
  const where = useWhere();
  const refresh = useRefreshReads();
  const tree = useResources(api).data?.item ?? [];
  const res = tree.find((r) => r.id === resource) ?? tree.find((r) => !r.parentId);
  const authorizers = useAuthorizers(api).data?.item ?? [];
  const taken = res ? methodsOf(res).map((m) => m.httpMethod) : [];
  const first = METHODS.find((m) => !taken.includes(m)) ?? "GET";
  const [f, setF] = useState<MethodForm>({ ...emptyMethod, method: first });
  const sentence = useFunctionSentence(f.fn);
  const [tried, setTried] = useState(false);
  const [error, setError] = useState<{ code: string; message: string; after: string | null } | null>(null);
  const [working, setWorking] = useState(false);
  const set = (patch: Partial<MethodForm>) => { setF((x) => ({ ...x, ...patch })); setError(null); };
  const w = { api, resource: res?.id ?? "", region: where.region, account: where.account };
  const calls = useMemo(() => methodCalls(f, w), [f, api, res?.id, where.region, where.account]); // eslint-disable-line react-hooks/exhaustive-deps
  const problem = methodProblem(f, taken);
  const submit = async () => {
    setTried(true);
    if (problem || working) return;
    setWorking(true);
    let step = "";
    try {
      await runCalls(REST, calls.map((c) => ({ operation: c.operation, input: c.input })), (o) => (step = o));
      refresh();
      onClose(`${f.method} ${res?.path ?? ""}`, res?.id);
    } catch (e) {
      refresh();
      setError({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : String(e), after: step === "PutMethod" ? null : step });
    } finally {
      setWorking(false);
    }
  };
  const lambda = f.kind === "lambda";
  return (
    <FormPage
      title="Create method"
      sub={<>Add a method to <span className="font-mono">{res?.path ?? "/"}</span> and connect it to a backend. The method request and the integration request are configured together.</>}
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Resources", to: `/resources/apigateway/restapis/${api}` }, { label: "Create method", to: `/resources/apigateway/restapis/${api}?do=create-method` }]}
      request={{ service: "apigateway", ...calls[0]! }}
      requests={calls.map((c) => ({ service: "apigateway", operation: c.operation, input: c.input }))}
      showInput
      shown={sdkShown}
      refusal={error}
      callTitle="The calls this will make"
      callNote={f.kind === "lambda" ? <>A Lambda integration always calls the function with <span className="font-mono">POST</span>, whatever the method type is. The proxy toggle chooses <span className="font-mono">AWS_PROXY</span> over <span className="font-mono">AWS</span>.</> : f.kind === "mock" ? <>A mock integration answers from API Gateway itself: the request template picks the status, the 200 responses carry the answer.</> : undefined}
      banner={error ? <Alert tone="bad" title={error.after ? `The method was made, but ${error.after} was refused.` : "The method wasn't created."} testId="create-refused">The service answered <span className="font-mono font-semibold text-bright">{error.code}</span>: “{error.message}”</Alert> : undefined}
      submitLabel="Create method"
      pending={working}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose(undefined, res?.id)}
      onSubmit={() => void submit()}
      testId="method-create"
    >
      <FormSection title="Method details" testId="section-details">
        <Field label="Method type" hint={res ? <>The HTTP verb callers use. <span className="font-mono">{res.path}</span> {taken.length === 0 ? "has no method yet" : <>already has {list(taken)}</>}.</> : undefined}>
          {(id) => (
            <Select value={f.method} onValueChange={(method) => set({ method })}>
              <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="method-type"><SelectValue /></SelectTrigger>
              <SelectContent>{METHODS.map((m) => <SelectItem key={m} value={m} disabled={taken.includes(m)} className="font-mono">{m}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Integration type</span>
          <RadioCards<IntegrationKind> name="integration" label="Integration type" value={f.kind} onChange={(kind) => set({ kind, proxy: kind === "lambda" || kind === "http" })} options={[
            { value: "lambda", title: "Lambda function", body: "Run a Lambda function." },
            { value: "http", title: "HTTP", body: "Forward to an HTTP endpoint." },
            { value: "mock", title: "Mock", body: "Answer from API Gateway itself." },
            { value: "aws", title: "AWS service", body: "Call an action of a service doze-aws runs." },
          ]} />
        </div>
        {(lambda || f.kind === "http") && (
          <div className="flex items-start gap-3">
            <Switch id="proxy" checked={f.proxy} onCheckedChange={(proxy) => set({ proxy })} className="mt-0.5" data-testid="method-proxy" />
            <div className="grid gap-0.5">
              <Label htmlFor="proxy">{lambda ? "Lambda proxy integration" : "HTTP proxy integration"}</Label>
              <p className="text-[13px] text-muted-foreground">{lambda ? <>API Gateway passes the incoming request from the client straight to the function, and the function builds the whole response. With the toggle off, the request and response go through mapping templates (Velocity), which doze-aws runs.</> : <>API Gateway passes the request to the endpoint as it is and returns the endpoint&apos;s answer. With the toggle off, the request and response go through mapping templates.</>}</p>
            </div>
          </div>
        )}
        {lambda && (
          <Field label="Lambda function" error={tried && !f.fn ? "Choose the Lambda function." : undefined} hint={<>The function&apos;s name, in {where.region}.{sentence && <> <Emphasis text={sentence} names={[f.fn ?? ""]} /></>} API Gateway must be allowed to invoke it, through its resource policy or an IAM role.</>}>
            {(id) => <FunctionPicker id={id} by="name" value={f.fn} onChange={(fn) => set({ fn })} />}
          </Field>
        )}
        {f.kind === "http" && (
          <>
            <Field label="Endpoint URL" error={tried && problem?.startsWith("Enter the endpoint") ? problem : undefined} help="A {param} placeholder is filled from the request path.">
              {(id) => <Input id={id} value={f.url} onChange={(e) => set({ url: e.target.value })} placeholder="https://catalog.larder.example.com/v1/products" className="font-mono" autoComplete="off" spellCheck={false} data-testid="method-url" />}
            </Field>
            <Field label="HTTP method" hint="The verb the endpoint is called with; the method's own when left as it is.">
              {(id) => (
                <Select value={f.httpMethod || (f.method === "ANY" ? "POST" : f.method)} onValueChange={(httpMethod) => set({ httpMethod })}>
                  <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="method-http-method"><SelectValue /></SelectTrigger>
                  <SelectContent>{METHODS.filter((m) => m !== "ANY").map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          </>
        )}
        {f.kind === "aws" && (
          <>
            <Field label="AWS service">
              {(id) => (
                <Select value={f.service} onValueChange={(service) => set({ service })}>
                  <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="method-service"><SelectValue /></SelectTrigger>
                  <SelectContent>{SERVICES.map((s) => <SelectItem key={s} value={s} className="font-mono">{s}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
            <Field label="Action" error={tried && f.action.trim() === "" ? "Enter the action to call." : undefined} help="An action of the service, such as SendMessage. The mapped request body is its input; a mapping template builds it.">
              {(id) => <Input id={id} value={f.action} onChange={(e) => set({ action: e.target.value })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="method-action" />}
            </Field>
            <Field label="Execution role" optional>{(id) => <RolePicker id={id} by="name" value={f.role} onChange={(role) => set({ role })} />}</Field>
          </>
        )}
      </FormSection>
      <FormSection title="Method request settings" collapsible defaultOpen={false} summary={<>Authorization {settingsSummary(f)} You can also configure these after you create the method.</>} testId="section-settings">
        <Field label="Authorization">
          {(id) => (
            <Select value={f.authorization} onValueChange={(authorization) => set({ authorization: authorization as MethodForm["authorization"], authorizer: authorization === "CUSTOM" ? (authorizers[0]?.id ?? null) : null })}>
              <SelectTrigger id={id} className="w-full bg-card" data-testid="method-authorization"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="NONE">None</SelectItem>
                <SelectItem value="AWS_IAM">IAM</SelectItem>
                {authorizers.length > 0 && <SelectItem value="CUSTOM">Authorizer</SelectItem>}
              </SelectContent>
            </Select>
          )}
        </Field>
        {f.authorization === "CUSTOM" && (
          <Field label="Authorizer">
            {(id) => (
              <Select value={f.authorizer ?? ""} onValueChange={(authorizer) => set({ authorizer })}>
                <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="method-authorizer"><SelectValue /></SelectTrigger>
                <SelectContent>{authorizers.map((a) => <SelectItem key={a.id} value={a.id} className="font-mono">{a.name}</SelectItem>)}</SelectContent>
              </Select>
            )}
          </Field>
        )}
        <div className="flex items-start gap-3"><Switch id="key" checked={f.apiKey} onCheckedChange={(apiKey) => set({ apiKey })} className="mt-0.5" data-testid="method-api-key" /><div className="grid gap-0.5"><Label htmlFor="key">API key required</Label><p className="text-[13px] text-muted-foreground">Serves the method only to a request whose <span className="font-mono">x-api-key</span> is enabled and in a usage plan that covers the stage.</p></div></div>
        <Field label="Operation name" optional>{(id) => <Input id={id} value={f.operation} onChange={(e) => set({ operation: e.target.value })} placeholder="ListProducts" className="font-mono" autoComplete="off" data-testid="method-operation" />}</Field>
        <RuleHelp>You can change all of these on the method request afterwards.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}
