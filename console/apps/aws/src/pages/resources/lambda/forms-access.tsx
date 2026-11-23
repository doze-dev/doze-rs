// Add permissions, Create function URL and Create alias (lambda__permission-add, url-create, alias-create): AddPermission,
// CreateFunctionUrlConfig and CreateAlias, each a page with the call it will make.
import { useMemo, useState } from "react";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, FormSection } from "../../../kit";
import { on, useOp, useOpRead } from "../deep/api";
import { type AliasForm, aliasInput, aliasProblems, permissionInput, permissionProblems, sourceHint, urlInput, urlProblems, type UrlForm } from "./forms";
import { FnFormPage, type FormProps, Muted } from "./form-bits";
import { usePolicy } from "./hooks";
import { urlAuthNote } from "./model";
import type { Alias, Version } from "./versions-tab";

const ACTIONS = ["lambda:InvokeFunction", "lambda:InvokeFunctionUrl", "lambda:GetFunction"];

export function PermissionAdd(p: FormProps) {
  const { d } = p;
  const policy = usePolicy(d.name);
  const [f, setF] = useState({ sid: "", principal: "", sourceArn: "", sourceAccount: "", action: ACTIONS[0] ?? "" });
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<typeof f>) => setF((x) => ({ ...x, ...patch }));
  const add = useOp(on.fn(d.name), "AddPermission");
  const taken = policy.statements.flatMap((s) => (s.Sid ? [s.Sid] : []));
  const problems = permissionProblems(f, taken);
  const input = useMemo(() => permissionInput(d.name, f), [d.name, f]);
  const hint = sourceHint(f.principal.trim(), d.name);
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    add.mutate({ ...input }, { onSuccess: () => p.go({ tab: "configuration", p: "permissions", done: "permission-added" }) });
  };
  const bucket = f.principal.trim() === "s3.amazonaws.com";
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "permissions" }} title="Add permissions" label="Add permissions" sub={<><b className="font-medium text-foreground">Let another account or an AWS service call {d.name}.</b> This adds one Allow statement to the function&apos;s resource-based policy.</>}
      request={f.sid || f.principal ? { service: "lambda", operation: "AddPermission", input } : null} refusal={add.error} callNote="The statement is added to the existing policy; nothing already there is replaced."
      submitLabel="Save" pending={add.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]} onSubmit={submit} testId="permission-form">
      <FormSection title="Resource-based policy" testId="section-permission">
        <Field label="Statement ID" error={tried || taken.includes(f.sid) ? problems.sid : undefined} help={<>A statement identifier that differentiates the statement from others in the same policy.{taken.length ? <Muted> {taken.join(", ")} {taken.length === 1 ? "is" : "are"} taken.</Muted> : null}</>} testId="field-sid">
          {(id) => <Input id={id} value={f.sid} onChange={(e) => set({ sid: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="perm-sid" />}
        </Field>
        <Field label="Principal" error={tried ? problems.principal : undefined} help={<>An AWS service, account, IAM user or IAM role. <Muted>If you name a service, use SourceArn or SourceAccount to limit who can invoke the function through that service.</Muted></>} testId="field-principal">
          {(id) => <Input id={id} value={f.principal} onChange={(e) => set({ principal: e.target.value })} placeholder="s3.amazonaws.com" autoComplete="off" spellCheck={false} className="font-mono" data-testid="perm-principal" />}
        </Field>
        <Field label={<>SourceArn <span className="font-normal text-muted-foreground">Optional</span></>} help={hint ?? "For an AWS service, the ARN of the resource that invokes the function."} testId="field-source-arn">
          {(id) => <Input id={id} value={f.sourceArn} onChange={(e) => set({ sourceArn: e.target.value })} placeholder={bucket ? "arn:aws:s3:::receipts" : ""} autoComplete="off" spellCheck={false} className="font-mono" data-testid="perm-source-arn" />}
        </Field>
        <Field label={<>SourceAccount <span className="font-normal text-muted-foreground">Optional</span></>} help={<>The ID of the account that owns the resource, used with SourceArn. <Muted>An Amazon S3 bucket can be deleted by its owner and recreated by another account.</Muted></>} testId="field-source-account">
          {(id) => <Input id={id} value={f.sourceAccount} onChange={(e) => set({ sourceAccount: e.target.value })} inputMode="numeric" autoComplete="off" className="max-w-[16rem] font-mono" data-testid="perm-source-account" />}
        </Field>
        <Field label="Action" help="The action the principal can use on the function." testId="field-action">
          {(id) => (
            <Select value={f.action} onValueChange={(action) => set({ action })}>
              <SelectTrigger id={id} className="w-full max-w-[22rem] bg-card font-mono" data-testid="perm-action"><SelectValue /></SelectTrigger>
              <SelectContent>{ACTIONS.map((a) => <SelectItem key={a} value={a} className="font-mono">{a}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
      </FormSection>
    </FnFormPage>
  );
}

export function UrlCreate(p: FormProps) {
  const { d } = p;
  const [u, setU] = useState<UrlForm>({ auth: "AWS_IAM", origins: "", methods: "", headers: "", maxAge: "" });
  const [tried, setTried] = useState(false);
  const [cors, setCors] = useState(false);
  const set = (patch: Partial<UrlForm>) => setU((x) => ({ ...x, ...patch }));
  const make = useOp(on.fn(d.name), "CreateFunctionUrlConfig");
  const problems = urlProblems(u);
  const input = useMemo(() => urlInput(d.name, cors ? u : { ...u, origins: "", methods: "", headers: "", maxAge: "" }), [d.name, u, cors]);
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    make.mutate({ ...input }, { onSuccess: () => p.go({ tab: "configuration", p: "url", done: "url-created" }) });
  };
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "url" }} title="Create function URL" label="Create function URL" sub={<><b className="font-medium text-foreground">Give {d.name} an HTTP(S) endpoint.</b> The URL is for the unpublished version, $LATEST.</>}
      request={{ service: "lambda", operation: "CreateFunctionUrlConfig", input }} refusal={make.error} callNote="No Qualifier, so the URL is for $LATEST."
      submitLabel="Save" pending={make.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]} onSubmit={submit} testId="url-form">
      <FormSection title="Function URL" testId="section-url">
        <div className="grid gap-1.5">
          <Label>Auth type</Label>
          <RadioGroup value={u.auth} onValueChange={(v) => set({ auth: v as UrlForm["auth"] })} className="grid gap-3" aria-label="Auth type">
            {[["AWS_IAM", "AWS_IAM", "Lambda uses IAM to authenticate and authorize requests. Choose this to let only authenticated users and roles invoke the function."], ["NONE", "NONE", "Lambda does not authenticate requests. The function's resource-based policy must grant public access before the URL can receive requests."]].map(([v, t, text]) => (
              <Label key={v} htmlFor={`auth-${v}`} className="flex cursor-pointer items-start gap-3 font-normal leading-5">
                <RadioGroupItem id={`auth-${v}`} value={v!} className="mt-0.5" data-testid={`auth-${v}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{t}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
              </Label>
            ))}
          </RadioGroup>
        </div>
        <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">The URL has the form https://&lt;url-id&gt;.lambda-url.&lt;region&gt;.on.aws and never changes. Function URLs are reached over the public internet only. <Muted>{urlAuthNote(u.auth)}</Muted></p>
        <div className="flex items-start gap-3">
          <Checkbox id="url-cors" checked={cors} onCheckedChange={(v) => setCors(v === true)} className="mt-0.5" data-testid="url-cors" />
          <Label htmlFor="url-cors" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Configure cross-origin resource sharing (CORS)</span><span className="text-[13px] text-muted-foreground">Choose which origins may call the URL from a browser.</span></Label>
        </div>
        {cors && (
          <>
            <Field label={<>AllowOrigins <span className="font-normal text-muted-foreground">Optional</span></>} help="Use * to allow every origin. Sent as the Access-Control-Allow-Origin header." testId="field-origins">
              {(id) => <Input id={id} value={u.origins} onChange={(e) => set({ origins: e.target.value })} placeholder="https://admin.larder.dev" autoComplete="off" spellCheck={false} className="font-mono" data-testid="url-origins" />}
            </Field>
            <Field label={<>AllowMethods <span className="font-normal text-muted-foreground">Optional</span></>} help="For example GET, POST, DELETE, or *." testId="field-methods">
              {(id) => <Input id={id} value={u.methods} onChange={(e) => set({ methods: e.target.value })} autoComplete="off" spellCheck={false} className="max-w-[16rem] font-mono" data-testid="url-methods" />}
            </Field>
            <Field label={<>AllowHeaders <span className="font-normal text-muted-foreground">Optional</span></>} help="The HTTP headers that origins can include in requests to the URL." testId="field-headers">
              {(id) => <Input id={id} value={u.headers} onChange={(e) => set({ headers: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="url-headers" />}
            </Field>
            <Field label={<>MaxAge <span className="font-normal text-muted-foreground">Optional</span></>} error={tried ? problems.maxAge : undefined} help={<>How long a browser may cache the result of a preflight request. <Muted>The API reference says the default is 0, which means the browser does not cache; the guide lists 5 as the default.</Muted></>} testId="field-max-age">
              {(id) => (
                <div className="relative w-full max-w-[16rem]">
                  <Input id={id} value={u.maxAge} onChange={(e) => set({ maxAge: e.target.value })} inputMode="numeric" autoComplete="off" className="pr-20 font-mono" data-testid="url-max-age" />
                  <span aria-hidden className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">seconds</span>
                </div>
              )}
            </Field>
          </>
        )}
      </FormSection>
    </FnFormPage>
  );
}

export function AliasCreate(p: FormProps) {
  const { d } = p;
  const versions = useOpRead<{ Versions?: Version[] }>(on.fn(d.name), "ListVersionsByFunction", { MaxItems: 50 });
  const aliases = useOpRead<{ Aliases?: Alias[] }>(on.fn(d.name), "ListAliases", { MaxItems: 50 });
  const numbers = (versions.data?.Versions ?? []).map((v) => v.Version).filter((v) => v !== "$LATEST").sort((a, b) => Number(a) - Number(b));
  const list = aliases.data?.Aliases ?? [];
  const [a, setA] = useState<AliasForm>({ name: "", description: "", version: null });
  const [extra, setExtra] = useState<string | null>(null);
  const [weight, setWeight] = useState("10");
  const [tried, setTried] = useState(false);
  const make = useOp(on.fn(d.name), "CreateAlias");
  const chosen = a.version ?? numbers[numbers.length - 1] ?? null;
  const form = { ...a, version: chosen };
  const problems = aliasProblems(form, list.map((x) => x.Name));
  const pct = Number(weight);
  const weightBad = extra !== null && (!Number.isFinite(pct) || pct <= 0 || pct >= 100);
  const input = { ...aliasInput(d.name, form), ...(extra && !weightBad ? { RoutingConfig: { AdditionalVersionWeights: { [extra]: pct / 100 } } } : {}) };
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0 || weightBad) return;
    make.mutate({ ...input }, { onSuccess: () => p.go({ tab: "aliases", done: "alias-created" }) });
  };
  const pointing = list.map((x) => `${x.Name} points at ${x.FunctionVersion}`);
  return (
    <FnFormPage p={{ ...p, tab: "aliases" }} title="Create alias" label="Create alias" sub={<><b className="font-medium text-foreground">Point a name at a version of {d.name}.</b></>}
      request={form.name ? { service: "lambda", operation: "CreateAlias", input } : null} refusal={make.error} callNote="FunctionVersion names the version the alias points to."
      submitLabel="Save" pending={make.isPending} disabled={tried && (Object.keys(problems).length > 0 || weightBad)} disabledWhy={Object.values(problems)[0] ?? (weightBad ? "A weight is a percentage between 0 and 100, exclusive." : undefined)} onSubmit={submit} testId="alias-form">
      <FormSection title="Alias configuration" testId="section-alias">
        <Field label="Name" error={tried || list.some((x) => x.Name === form.name) ? problems.name : undefined} help={<>Letters, digits, hyphens and underscores; a name made only of digits is not allowed.{list.length ? <Muted> {list.map((x) => x.Name).join(" and ")} already exist{list.length === 1 ? "s" : ""}.</Muted> : null}</>} testId="field-name">
          {(id) => <Input id={id} value={a.name} onChange={(e) => setA({ ...a, name: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="alias-name" />}
        </Field>
        <Field label={<>Description <span className="font-normal text-muted-foreground">Optional</span></>} error={tried ? problems.description : undefined} help="Up to 256 characters." testId="field-description">
          {(id) => <Input id={id} value={a.description} onChange={(e) => setA({ ...a, description: e.target.value })} autoComplete="off" data-testid="alias-description" />}
        </Field>
        <Field label="Version" error={tried ? problems.version : undefined} help={<>The version the alias points to. <Muted>{d.name} has {numbers.length === 0 ? "no published version yet" : <>version{numbers.length === 1 ? "" : "s"} {numbers.join(", ")}</>}{pointing.length ? `; ${pointing.join(", ")}` : ""}.</Muted></>} testId="field-version">
          {(id) => (
            <Select value={chosen ?? undefined} onValueChange={(version) => setA({ ...a, version })}>
              <SelectTrigger id={id} className="w-full max-w-[16rem] bg-card font-mono" data-testid="alias-version"><SelectValue placeholder="Choose a version" /></SelectTrigger>
              <SelectContent>{numbers.map((n) => <SelectItem key={n} value={n} className="font-mono">{n}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
      </FormSection>
      <FormSection title="Weighted alias" collapsible defaultOpen={false} description="Send a share of the alias's traffic to a second version. doze-aws routes each invocation by these weights." testId="section-weighted">
        <Field label="Additional version" help="The version that gets the share; the rest goes to the version above." testId="field-extra">
          {(id) => (
            <Select value={extra ?? "none"} onValueChange={(v) => setExtra(v === "none" ? null : v)}>
              <SelectTrigger id={id} className="w-full max-w-[16rem] bg-card font-mono" data-testid="alias-extra"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="none">None</SelectItem>{numbers.filter((n) => n !== chosen).map((n) => <SelectItem key={n} value={n} className="font-mono">{n}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        {extra && (
          <Field label="Weight" error={tried && weightBad ? "A weight is a percentage between 0 and 100, exclusive." : undefined} help="The percentage of invocations the additional version gets." testId="field-weight">
            {(id) => (
              <div className="relative w-full max-w-[12rem]"><Input id={id} value={weight} onChange={(e) => setWeight(e.target.value)} inputMode="decimal" className="pr-10 font-mono" data-testid="alias-weight" /><span aria-hidden className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">%</span></div>
            )}
          </Field>
        )}
      </FormSection>
    </FnFormPage>
  );
}
