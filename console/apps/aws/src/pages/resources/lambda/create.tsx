// Create Event Function (lambda__function-create): a page, not a dialog. Author from scratch with the Hello world example for the
// chosen runtime, an execution role, and the optional URL and tags; the call it makes is beside it. CreateFunction is made as the
// console with the example zipped in the page; a URL asked for is a second call (CreateFunctionUrlConfig) once the function exists.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { BoxIcon, FileArchiveIcon, LayoutTemplateIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { useResourceList } from "../../../api/resources";
import { callOp } from "../deep/api";
import { Field, FormPage, FormSection, RolePicker, useKindOp } from "../../../kit";
import { type Create, createProblems } from "./forms";
import { FUNCTIONS, refusalOf, useWhere } from "./hooks";
import { familyOf, HELLO, roleArn, runtimeName, RUNTIMES, tailOf } from "./model";
import { buildZip, toBase64 } from "./zip";

const OFF = [
  { icon: BoxIcon, title: "Container image", why: "Not available in doze-aws: functions run as local processes and no images are pulled." },
  { icon: LayoutTemplateIcon, title: "Use a blueprint", why: "Not available in doze-aws: blueprints come from a catalog the AWS console hosts." },
];

/** The archive of the Hello world example for a runtime, as the page sends it. */
export function helloZip(runtime: string): string {
  const h = HELLO[familyOf(runtime)];
  return toBase64(buildZip([{ path: h.file, data: new TextEncoder().encode(h.source), mode: 0o644 }]));
}

export function FunctionCreate() {
  const navigate = useNavigate();
  const where = useWhere();
  const [c, setC] = useState<Create>({ name: "", runtime: "python3.13", arch: "x86_64", role: null });
  const [urlOn, setUrlOn] = useState(false);
  const [urlAuth, setUrlAuth] = useState<"AWS_IAM" | "NONE">("AWS_IAM");
  const [tags, setTags] = useState<KeyValueRow[]>([]);
  const [tried, setTried] = useState(false);
  const [urlError, setUrlError] = useState<unknown>(null);
  const set = (patch: Partial<Create>) => setC((x) => ({ ...x, ...patch }));
  const functions = useResourceList("lambda", "functions");
  const rows = functions.data?.pages.flatMap((p) => p.rows) ?? [];
  const taken = rows.map((r) => r.name);
  const make = useKindOp<{ FunctionName: string }>(FUNCTIONS, "CreateFunction");
  const problems = createProblems(c, taken);
  const handler = HELLO[familyOf(c.runtime)].handler;
  const filled = tags.filter((t) => t.key);
  const body = (zip: string) => ({ FunctionName: c.name, Runtime: c.runtime, Role: c.role ?? "", Handler: handler, Code: { ZipFile: zip }, Architectures: [c.arch], PackageType: "Zip", ...(filled.length ? { Tags: Object.fromEntries(filled.map((t) => [t.key, t.value])) } : {}) });
  const shown = body("<the Hello world example, zipped>");
  const refused = refusalOf(make.error);
  const used = rows.slice(0, 3);
  const runtimeIn = (id: string) => rows.find((r) => r.cells[2] === id)?.name;
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    setUrlError(null);
    make.mutate(body(helloZip(c.runtime)), {
      onSuccess: async (r) => {
        if (urlOn) {
          try {
            await callOp({ service: "lambda", kind: "functions", name: r.FunctionName }, "CreateFunctionUrlConfig", { AuthType: urlAuth });
          } catch (e) {
            setUrlError(e);
          }
        }
        void navigate({ to: "/resources/$service/$kind/$name", params: { service: "lambda", kind: "functions", name: r.FunctionName }, search: { done: urlOn ? "created" : "created" } });
      },
    });
  };
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "lambda", second: "functions" }, search: {} });
  return (
    <FormPage
      title="Create Event Function"
      sub="Name the function and choose its runtime. The new function is a Hello world function that you can change on the Code tab."
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Compute and flow", to: "/resources" }, { label: "Lambda", to: "/resources/lambda" }, { label: "Event Functions", to: "/resources/lambda/functions" }]}
      request={c.name ? { service: "lambda", operation: "CreateFunction", input: shown } : null}
      showInput
      refusal={refused}
      callNote="Runtime and Handler are required for a .zip package; the console fills in the handler that matches its example."
      banner={refused ? <Alert tone="bad" title="Function not created." testId="create-refused">{refused.code}: {refused.message}</Alert> : urlError ? <Alert tone="warn" title="The function was created, but its URL was not.">{(urlError as Error).message}</Alert> : undefined}
      submitLabel="Create Event Function"
      pending={make.isPending}
      disabled={tried && Object.keys(problems).length > 0}
      disabledWhy={Object.values(problems)[0]}
      onCancel={back}
      onSubmit={submit}
      testId="function-create"
    >
      <FormSection title="Author from scratch" description="Start with a Hello world example. The package is a .zip file archive." testId="section-author">
        <div className="grid gap-3 md:grid-cols-2">
          {OFF.map((o) => (
            <div key={o.title} className="flex gap-3 rounded-xl border border-dashed p-4 text-[13.5px] opacity-80" data-testid="author-off">
              <o.icon aria-hidden className="mt-0.5 size-5 shrink-0 text-muted-foreground" />
              <div><p className="font-medium text-bright">{o.title}</p><p className="mt-0.5 text-muted-foreground">{o.why}</p></div>
            </div>
          ))}
        </div>
        <p className="flex items-center gap-2 border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground"><FileArchiveIcon aria-hidden className="size-4" />The package type cannot be changed after the function exists: a .zip function stays .zip.</p>
      </FormSection>
      <FormSection title="Basic information" testId="section-basic">
        <Field label="Function name" error={tried || taken.includes(c.name) ? problems.name : undefined} help={<>Function names are limited to <b>64</b> characters.{used.length ? <span className="text-muted-foreground"> {used.map((r) => r.name).join(", ")} already exist{used.length === 1 ? "s" : ""} in this Region.</span> : null}</>} testId="field-name">
          {(id) => <Input id={id} value={c.name} onChange={(e) => set({ name: e.target.value })} placeholder="weekly-digest" autoComplete="off" spellCheck={false} className="font-mono" data-testid="fn-name" />}
        </Field>
        <Field label="Runtime" help={<>A runtime is required for a .zip function.{runtimeIn("python3.13") ? <span className="text-muted-foreground"> {runtimeIn("python3.13")} uses python3.13{runtimeIn("nodejs22.x") ? <> and {runtimeIn("nodejs22.x")} nodejs22.x</> : null}.</span> : null} <span className="text-muted-foreground">Locally the function runs on the {familyOf(c.runtime) === "node" ? "node" : familyOf(c.runtime) === "ruby" ? "ruby" : "python3"} found on this machine.</span></>} testId="field-runtime">
          {(id) => (
            <Select value={c.runtime} onValueChange={(runtime) => set({ runtime })}>
              <SelectTrigger id={id} className="w-full max-w-[22rem] bg-card" data-testid="fn-runtime"><SelectValue>{runtimeName(c.runtime)}</SelectValue></SelectTrigger>
              <SelectContent>{RUNTIMES.map((r) => <SelectItem key={r.id} value={r.id}>{r.label}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <div className="grid gap-1.5">
          <Label>Architecture <span className="font-normal text-muted-foreground">Optional</span></Label>
          <RadioGroup value={c.arch} onValueChange={(v) => set({ arch: v as Create["arch"] })} className="flex gap-6" aria-label="Architecture">
            {(["x86_64", "arm64"] as const).map((a) => (
              <Label key={a} htmlFor={`arch-${a}`} className="flex cursor-pointer items-center gap-2 font-mono font-normal"><RadioGroupItem id={`arch-${a}`} value={a} data-testid={`arch-${a}`} />{a}</Label>
            ))}
          </RadioGroup>
          <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">The default architecture is x86_64. The code you upload must match the instruction set.</p>
        </div>
      </FormSection>
      <FormSection title="Permissions" description="Choose the execution role Lambda assumes to run the function." testId="section-permissions">
        <div className="grid gap-2">
          <div className="rounded-lg border border-dashed px-4 py-3 text-[13.5px] text-muted-foreground opacity-80" data-testid="role-create-off"><span className="font-medium text-foreground">Create default role</span><br />Not in this form: create the role in IAM, then choose it here.</div>
          <div className="rounded-lg border border-primary bg-pick/40 px-4 py-3 text-[13.5px] font-medium text-bright">Use another role</div>
        </div>
        <Field label="Execution role" error={tried ? problems.role : undefined} help={<>The role&apos;s ARN is sent as <span className="font-mono">Role</span>, which CreateFunction requires. <span className="text-muted-foreground">{c.role ? <><span className="font-mono">{tailOf(c.role)}</span> becomes the role the function&apos;s process gets credentials for.</> : "The function’s process gets credentials for this role."}</span></>} testId="field-role">
          {(id) => <RolePicker id={id} value={c.role ? tailOf(c.role) : null} onChange={(role) => set({ role: role ? roleArn(where.account, role) : null })} by="id" className="w-full max-w-[22rem] font-mono" />}
        </Field>
      </FormSection>
      <section className="rounded-[14px] border bg-card p-6 lg:p-7" data-testid="section-advanced">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Advanced settings</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">Open a section to configure it while you create the function.</p>
      </section>
      <FormSection title="Function URL" collapsible defaultOpen={false} description={<>Gives the function its own HTTPS endpoint. {urlOn ? `On, with ${urlAuth}.` : "Off."}</>} testId="section-url">
        <div className="flex items-center gap-3">
          <Switch id="fn-url" checked={urlOn} onCheckedChange={setUrlOn} data-testid="fn-url" />
          <Label htmlFor="fn-url" className="font-normal">Enable function URL</Label>
        </div>
        {urlOn && (
          <Field label="Auth type" help="AWS_IAM lets only authenticated users and roles invoke the function; NONE lets anyone. CORS can be set on the function's Function URL section." testId="field-auth">
            {(id) => (
              <Select value={urlAuth} onValueChange={(v) => setUrlAuth(v as "AWS_IAM" | "NONE")}>
                <SelectTrigger id={id} className="w-full max-w-[16rem] bg-card font-mono" data-testid="fn-url-auth"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="AWS_IAM" className="font-mono">AWS_IAM</SelectItem><SelectItem value="NONE" className="font-mono">NONE</SelectItem></SelectContent>
              </Select>
            )}
          </Field>
        )}
      </FormSection>
      <FormSection title="Enable tags" collapsible defaultOpen={false} description={filled.length ? `${filled.length} ${filled.length === 1 ? "tag" : "tags"}.` : "No tags."} testId="section-tags">
        <KeyValueEditor rows={tags} onChange={setTags} rules={{ maxKey: 128, maxValue: 256, maxRows: 50 }} addLabel="Add new tag" testId="fn-tags" />
        {tags.length === 0 && <button type="button" className="w-fit rounded-md border bg-card px-3 py-1.5 text-[13px] hover:bg-accent" onClick={() => setTags([newRow()])} data-testid="fn-tags-start">Add new tag</button>}
      </FormSection>
      <FormSection title="Enable VPC" collapsible defaultOpen={false} description="Not available in doze-aws: there is no VPC network to attach a function to." testId="section-vpc">
        <p className="text-[13.5px] text-muted-foreground">A function runs as a process on this machine, so it has no subnets or security groups to be placed in.</p>
      </FormSection>
    </FormPage>
  );
}
