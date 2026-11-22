// Create connection (eventbridge__connection-create): its authorization type and credentials, and the HTTP parameters every request carries. It is
// reached from Create API destination and goes back to it with the new connection chosen.
import { PlusIcon, XIcon } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { newRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Field, FormPage, FormSection, RadioCards, RuleHelp, useServiceCrumbs } from "../../../kit";
import { DESTS, useConnections, useEbOp } from "./api";
import { type AuthType, type ConnForm, type HttpParam, type ParamKind, METHODS, connProblem, createConnInput, emptyConn, nameProblem } from "./dest-model";
import { list } from "./words";

const AUTHS = [{ value: "API_KEY", label: "API Key" }, { value: "BASIC", label: "Basic (username/password)" }, { value: "OAUTH_CLIENT_CREDENTIALS", label: "OAuth Client Credentials" }];

export function ConnectionCreate() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const [f, setF] = useState<ConnForm>(emptyConn);
  const [tried, setTried] = useState(false);
  const make = useEbOp<{ ConnectionArn: string }>(DESTS, "CreateConnection");
  const others = (useConnections().data?.Connections ?? []).map((c) => c.Name);
  const problem = connProblem(f, others);
  const set = (p: Partial<ConnForm>) => { setF((x) => ({ ...x, ...p })); make.reset(); };
  const input = useMemo(() => createConnInput(f), [f]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const setParam = (id: string, p: Partial<HttpParam>) => set({ params: f.params.map((x) => (x.id === id ? { ...x, ...p } : x)) });
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "api-destinations" }, search: { create: "yes" } });
  return (
    <FormPage
      title="Create connection"
      sub="Name it, choose how to authorize, and give the credentials. On AWS they go into a Secrets Manager secret; doze-aws keeps them in the connection record and never returns them."
      crumbs={[...crumbs, { label: "API destinations", to: "/resources/eventbridge/api-destinations" }, { label: "Create API destination", to: "/resources/eventbridge/api-destinations?create=yes" }]}
      request={{ service: "events", operation: "CreateConnection", input }}
      refusal={refused}
      callNote="The reply holds the connection ARN and its state. Wait for Authorized before you create an API destination on it: here it is Authorized at once."
      banner={refused ? <Alert tone="bad" title="The connection wasn't created." testId="create-refused">CreateConnection answered <span className="font-mono font-semibold text-bright">{refused.code}</span>: “{refused.message}”</Alert> : undefined}
      submitLabel="Create Connection"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={back}
      onSubmit={() => {
        setTried(true);
        if (!problem) make.mutate(input as never, { onSuccess: () => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "api-destinations" }, search: { create: "yes", name: f.name } }) });
      }}
      testId="connection-create"
    >
      <FormSection title="Connection detail" testId="section-detail">
        <Field label="Name" error={tried ? (nameProblem(f.name, "connection") ?? (others.includes(f.name) ? `${f.name} already exists.` : undefined) ?? undefined) : undefined} help={<>Up to <b>64</b> characters: letters, digits, dot, hyphen and underscore.</>} hint={others.length > 0 ? <><b className="font-mono font-semibold text-foreground">{list(others.slice(0, 3))}</b> exist{others.length === 1 ? "s" : ""}; {others.length === 1 ? "it is" : "they are"} the other connection{others.length === 1 ? "" : "s"} an API destination can pick.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="status-key" autoComplete="off" spellCheck={false} className="font-mono" data-testid="conn-name" />}
        </Field>
        <Field label="Description" optional help={<>Up to <b>512</b> characters.</>}>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="conn-description" />}</Field>
        <div className="grid gap-1.5">
          <Label>Destination type</Label>
          <RadioCards<"public" | "private"> name="dest-type" label="Destination type" value="public" onChange={() => undefined} options={[{ value: "public", title: "Public", body: "A public HTTPS endpoint." }, { value: "private", title: "Private", body: "Not available in doze-aws: private APIs need VPC Lattice.", disabled: true }]} />
        </div>
      </FormSection>
      <FormSection title="Authorization" description="EventBridge builds the authorization headers for Basic and API key. For OAuth it also swaps the client ID and secret for a token and keeps it fresh." testId="section-auth">
        <div className="grid gap-1.5">
          <Label htmlFor="auth-type">Authorization type</Label>
          <Combobox id="auth-type" options={AUTHS} value={f.auth} onChange={(a) => a && set({ auth: a as AuthType })} aria-label="Authorization type" className="w-full" />
          <p className="text-[13px] text-muted-foreground">The other choices are Basic (username and password) and OAuth Client Credentials.</p>
        </div>
        {f.auth === "API_KEY" && (
          <div className="grid gap-4 md:grid-cols-2">
            <RuleHelp className="md:order-last md:col-span-2">EventBridge sends the value in the header you name.</RuleHelp>
            <Field label="API key name" error={tried && !f.keyName ? "Give the header's name." : undefined}>{(id) => <Input id={id} value={f.keyName} onChange={(e) => set({ keyName: e.target.value })} autoComplete="off" className="font-mono" data-testid="conn-key-name" />}</Field>
            <Field label="Value" error={tried && !f.keyValue ? "Give the value." : undefined}>{(id) => <Input id={id} type="password" value={f.keyValue} onChange={(e) => set({ keyValue: e.target.value })} autoComplete="off" data-testid="conn-key-value" />}</Field>
          </div>
        )}
        {f.auth === "BASIC" && (
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Username" error={tried && !f.username ? "Give the username." : undefined}>{(id) => <Input id={id} value={f.username} onChange={(e) => set({ username: e.target.value })} autoComplete="off" data-testid="conn-username" />}</Field>
            <Field label="Password" error={tried && !f.password ? "Give the password." : undefined}>{(id) => <Input id={id} type="password" value={f.password} onChange={(e) => set({ password: e.target.value })} autoComplete="off" data-testid="conn-password" />}</Field>
          </div>
        )}
        {f.auth === "OAUTH_CLIENT_CREDENTIALS" && (
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Authorization endpoint" className="md:col-span-2">{(id) => <Input id={id} value={f.tokenEndpoint} onChange={(e) => set({ tokenEndpoint: e.target.value })} placeholder="https://auth.larder.example/token" autoComplete="off" className="font-mono" data-testid="conn-token-endpoint" />}</Field>
            <Field label="Client ID">{(id) => <Input id={id} value={f.clientId} onChange={(e) => set({ clientId: e.target.value })} autoComplete="off" data-testid="conn-client-id" />}</Field>
            <Field label="Client secret">{(id) => <Input id={id} type="password" value={f.clientSecret} onChange={(e) => set({ clientSecret: e.target.value })} autoComplete="off" data-testid="conn-client-secret" />}</Field>
            <RuleHelp className="md:col-span-2">An OAuth connection is stored and returned, and its token is not requested: events to an API destination on it are not delivered. Use API key or Basic to deliver.</RuleHelp>
          </div>
        )}
        {problem && tried && <p className="text-[13px] text-bad" role="alert" data-testid="conn-problem">{problem}</p>}
      </FormSection>
      <FormSection title="Invocation HTTP parameters" optional description="Extra header, query string or body values sent with every request that uses this connection." testId="section-params">
        <ul className="grid gap-3" data-testid="http-params">
          {f.params.map((p) => (
            <li key={p.id} className="grid items-end gap-3 md:grid-cols-[9rem_minmax(0,1fr)_minmax(0,1fr)_auto_auto]">
              <div className="grid gap-1.5"><Label className="text-[12.5px] font-normal text-muted-foreground">Parameter</Label><Combobox options={[{ value: "header", label: "Header" }, { value: "query", label: "Query string" }, { value: "body", label: "Body field" }]} value={p.kind} onChange={(k) => k && setParam(p.id, { kind: k as ParamKind })} aria-label="Parameter type" className="w-full" /></div>
              <div className="grid gap-1.5"><Label className="text-[12.5px] font-normal text-muted-foreground">Key</Label><Input value={p.key} onChange={(e) => setParam(p.id, { key: e.target.value })} autoComplete="off" className="font-mono" aria-label="Key" data-testid="param-key" /></div>
              <div className="grid gap-1.5"><Label className="text-[12.5px] font-normal text-muted-foreground">Value</Label><Input value={p.value} onChange={(e) => setParam(p.id, { value: e.target.value })} autoComplete="off" type={p.secret ? "password" : "text"} aria-label="Value" data-testid="param-value" /></div>
              <Label className="pb-2 font-normal"><Checkbox checked={p.secret} onCheckedChange={(on) => setParam(p.id, { secret: on === true })} /> Secret</Label>
              <Button type="button" variant="ghost" size="icon" aria-label="Remove parameter" onClick={() => set({ params: f.params.filter((x) => x.id !== p.id) })}><XIcon /></Button>
            </li>
          ))}
        </ul>
        <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ params: [...f.params, { ...newRow(), kind: "header", secret: false }] })} data-testid="add-param"><PlusIcon /> Add parameter</Button></div>
        <RuleHelp>If a rule target sets the same key in its own HTTP parameters, the value from the connection wins. Methods for the OAuth token request: {METHODS.slice(0, 3).join(", ")}.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}
