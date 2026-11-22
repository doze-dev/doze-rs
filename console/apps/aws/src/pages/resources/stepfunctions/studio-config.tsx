// Workflow Studio's Config mode (stepfunctions__machine-create-config): the machine's name, type, execution role, logging and the
// optional settings. Everything here is one object (`Config`, studio-model.ts) the studio keeps, so the call beside it is drawn from it.
import { ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { type KeyValueRow, rowsFrom } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Field, FormSection, KeyPicker, LocalStatus, RadioCards, RolePicker, RuleHelp, TAG_RULES } from "../../../kit";
import { CodeBlock } from "../deep/parts";
import { LOG_NAME, existingWords, nameProblem, pretty, roleNameOf, TRUST } from "./model";
import { type Config, type Level, type RoleChoice, type Where, logGroupOf } from "./studio-model";
import type { Definition } from "./graph";
import { permissionsOf } from "./model";

const NAME_RULE = <>1 to 80 characters. No whitespace, wildcard characters ( ? * ), brackets ( &lt; &gt; &#123; &#125; [ ] ) or special characters ( &quot; # % \ ^ | ~ ` $ &amp; , ; : / ). To send logs to CloudWatch, use only <span className="font-mono">0-9 A-Z a-z - _ .</span> <b className="font-semibold">The name and the type can't be changed after you create it.</b></>;

const rowsOf = (tags: Config["tags"]): KeyValueRow[] => rowsFrom(tags.length > 0 ? tags : [{ key: "", value: "" }]);

export function ConfigMode({ name, setName, cfg, set, taken, where, definition, editing, tried }: { name: string; setName: (n: string) => void; cfg: Config; set: (c: Partial<Config>) => void; taken: readonly string[]; where: Where; definition: Definition | undefined; editing: boolean; tried: boolean }) {
  const [review, setReview] = useState(false);
  const nameError = editing ? null : nameProblem(name, taken);
  const exists = nameError?.endsWith("already exists in this Region.");
  const roleName = roleNameOf(name || "name");
  const policy = permissionsOf(definition, where);
  const [rows, setRows] = useState<KeyValueRow[]>(() => rowsOf(cfg.tags));
  const roleOptions: { value: RoleChoice; title: string; body: string }[] = [
    ...(editing ? [] : [{ value: "create" as const, title: "Create new role", body: "Step Functions creates an execution role for this state machine using least privileges." }]),
    { value: "existing", title: "Choose an existing role", body: "Pick one from your account, such as larder-admin." },
    { value: "arn", title: "Enter a role ARN", body: "Use a role by its ARN." },
  ];
  return (
    <div className="grid min-w-0 gap-6" data-testid="studio-config">
      <FormSection title="Details">
        <div className="grid gap-6">
          <Field label="State machine name" error={tried && nameError && !exists ? nameError : undefined} refusal={tried && exists ? { why: nameError } : null} help={NAME_RULE} hint={editing ? undefined : existingWords(taken.slice(0, 4))}>
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} readOnly={editing} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="config-name" />}
          </Field>
          <div className="grid gap-1.5">
            <Label>Type</Label>
            <RadioCards value={cfg.type} onChange={(type) => set({ type })} label="Type" name="type" disabled={editing} options={[
              { value: "STANDARD", title: "Standard", body: "Runs up to one year. Each step runs exactly once. Keeps the execution history." },
              { value: "EXPRESS", title: "Express", body: "Runs up to five minutes. Steps run at least once. Keeps history only in CloudWatch Logs." },
            ]} />
          </div>
        </div>
      </FormSection>
      <FormSection title="Permissions">
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label id="role-label">Execution role</Label>
            <RadioGroup value={cfg.role} onValueChange={(v) => set({ role: v as RoleChoice, roleArn: "" })} aria-labelledby="role-label" className="grid gap-3" data-testid="role-choice">
              {roleOptions.map((o) => (
                <Label key={o.value} htmlFor={`role-${o.value}`} className="flex items-start gap-3 font-normal leading-5">
                  <RadioGroupItem id={`role-${o.value}`} value={o.value} className="mt-0.5" data-testid={`role-${o.value}`} />
                  <span className="grid gap-0.5"><span className="font-medium text-bright">{o.title}</span><span className="text-[13px] text-muted-foreground">{o.body}</span></span>
                </Label>
              ))}
            </RadioGroup>
          </div>
          {cfg.role === "create" && (
            <>
              <Callout tone="note" testId="role-create-note">An execution role will be created with the permissions its tasks need. Role to create: <span className="font-mono">{roleName}</span></Callout>
              <div>
                <Button type="button" variant="outline" className="bg-card" onClick={() => setReview(!review)} aria-expanded={review} data-testid="role-review"><ChevronRightIcon className={review ? "rotate-90" : ""} /> Review auto-generated permissions</Button>
                {review && (
                  <div className="mt-3 grid gap-3" data-testid="role-review-body">
                    <p className="text-[13px] text-muted-foreground">The trust policy lets Step Functions assume the role.</p>
                    <CodeBlock text={pretty(TRUST)} max="max-h-60" />
                    <p className="text-[13px] text-muted-foreground">{policy ? "The permissions policy gives it what the definition's tasks call, on what they name, and nothing else." : "The definition has no task that calls a service, so the role has no permissions policy."}</p>
                    {policy && <CodeBlock text={pretty(policy)} max="max-h-72" />}
                  </div>
                )}
              </div>
            </>
          )}
          {cfg.role === "existing" && (
            <Field label="Role" error={tried && cfg.roleArn === "" ? "Choose an execution role." : undefined}>
              {(id) => <RolePicker id={id} value={cfg.roleArn || null} onChange={(roleArn) => set({ roleArn: roleArn ?? "" })} by="id" className="max-w-[28rem]" />}
            </Field>
          )}
          {cfg.role === "arn" && (
            <Field label="Role ARN" error={tried && !/^arn:aws[a-z-]*:iam::\d{12}:role\/.+/.test(cfg.roleArn.trim()) ? "Enter the role's ARN (arn:aws:iam::<account>:role/<name>)." : undefined}>
              {(id) => <Input id={id} value={cfg.roleArn} onChange={(e) => set({ roleArn: e.target.value })} placeholder={`arn:aws:iam::${where.account}:role/my-role`} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="role-arn" />}
            </Field>
          )}
        </div>
      </FormSection>
      <FormSection title="Logging">
        <div className="grid gap-5">
          <Field label="Log level" help={<><b className="font-semibold">ALL</b> logs every event, <b className="font-semibold">ERROR</b> logs failures, <b className="font-semibold">FATAL</b> logs only executions that fail, abort or time out, and <b className="font-semibold">OFF</b> logs nothing. A Standard machine made in the console doesn't log by default; an Express one does. <span className="text-muted-foreground">CloudWatch Logs charges the vended-logs rate for what is sent.</span></>}>
            {(id) => (
              <Select value={cfg.level} onValueChange={(v) => set({ level: v as Level })}>
                <SelectTrigger id={id} className="w-full max-w-[18rem] bg-card" data-testid="config-level"><SelectValue /></SelectTrigger>
                <SelectContent>{(["OFF", "ALL", "ERROR", "FATAL"] as const).map((l) => <SelectItem key={l} value={l}>{l}</SelectItem>)}</SelectContent>
              </Select>
            )}
          </Field>
          {cfg.level !== "OFF" && <p className="text-[13px] text-muted-foreground" data-testid="config-log-group">Events go to the log group <span className="font-mono text-foreground">{logGroupOf(name || "name")}</span>, which the studio makes if it is not there.</p>}
          <div className="flex items-start gap-3">
            <Checkbox id="config-data" checked={cfg.data} onCheckedChange={(c) => set({ data: c === true })} disabled={cfg.level === "OFF"} className="mt-0.5" data-testid="config-data" />
            <div className="grid gap-0.5"><Label htmlFor="config-data" className="font-mono">includeExecutionData</Label><p className="text-[13px] text-muted-foreground">Whether execution data is included in the log. When it is off, data is excluded.</p></div>
          </div>
        </div>
      </FormSection>
      <FormSection title="Additional configuration" optional>
        <div className="grid gap-6">
          <Toggle id="config-tracing" checked={cfg.tracing} onChange={(tracing) => set({ tracing })} title="Enable X-Ray tracing" body="Sends traces for executions even when the caller passes no trace ID.">
            <LocalStatus state="stored" className="mt-0.5" />
          </Toggle>
          {!editing && <Toggle id="config-publish" checked={cfg.publish} onChange={(publish) => set({ publish })} title="Publish version on creation" body="Publishes version 1 as the first revision, so you can start it by version or alias." />}
          <Toggle id="config-kms" checked={cfg.kms} onChange={(kms) => set({ kms })} title="Encrypt with customer managed key" body="Adds a second layer over the AWS owned key that always encrypts data at rest. The data key reuse period is 60 to 900 seconds.">
            <LocalStatus state="stored" className="mt-0.5" />
          </Toggle>
          {cfg.kms && (
            <div className="grid gap-5 pl-14 sm:grid-cols-2">
              <Field label="KMS key">{(id) => <KeyPicker id={id} value={cfg.kmsKey} onChange={(kmsKey) => set({ kmsKey })} by="id" />}</Field>
              <Field label="Data key reuse period" hint="Seconds, 60 to 900.">{(id) => <Input id={id} inputMode="numeric" value={cfg.kmsPeriod} onChange={(e) => set({ kmsPeriod: e.target.value })} className="bg-card" data-testid="config-kms-period" />}</Field>
            </div>
          )}
          {!editing && (
            <div className="grid gap-4">
              <div className="flex items-start gap-3">
                <Checkbox id="config-tags" checked={cfg.tagsOn} onCheckedChange={(c) => set({ tagsOn: c === true })} className="mt-0.5" data-testid="config-tags" />
                <div className="grid gap-0.5"><Label htmlFor="config-tags">Tags</Label><p className="text-[13px] text-muted-foreground">Tags help you track and manage the costs of your resources and give better security in IAM policies.</p></div>
              </div>
              {cfg.tagsOn && (
                <>
                  <KeyValueEditor rows={rows} onChange={(r) => { setRows(r); set({ tags: r.map((x) => ({ key: x.key, value: x.value })) }); }} rules={TAG_RULES} addLabel="Add tag" testId="config-tags-kv" />
                  <RuleHelp>Up to 50 tags per state machine. Keys are 1 to 128 characters, values 0 to 256. Letters, digits, spaces and <span className="font-mono">_ . : / = + - @</span> only; <span className="font-mono">aws:</span> is reserved.</RuleHelp>
                </>
              )}
            </div>
          )}
        </div>
      </FormSection>
    </div>
  );
}

function Toggle({ id, checked, onChange, title, body, children }: { id: string; checked: boolean; onChange: (v: boolean) => void; title: string; body: string; children?: React.ReactNode }) {
  return (
    <div className="flex items-start gap-3.5">
      <Switch id={id} checked={checked} onCheckedChange={onChange} className="mt-0.5" data-testid={id} />
      <div className="grid gap-0.5"><Label htmlFor={id} className="font-medium text-bright">{title}</Label><p className="text-[13px] leading-5 text-muted-foreground">{body}</p>{children}</div>
    </div>
  );
}

export { LOG_NAME };
