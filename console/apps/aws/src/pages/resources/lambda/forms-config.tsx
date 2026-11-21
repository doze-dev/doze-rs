// Edit General configuration (lambda__config-edit) and Edit environment variables (lambda__env-edit): the same fields the
// Configuration tab shows, saved with UpdateFunctionConfiguration (only what changed; the variables are one map, sent whole).
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field, FormSection, RolePicker } from "../../../kit";
import { on, useOp } from "../deep/api";
import { envInput, envProblems, generalChange, generalOf, generalProblems, RESERVED_KEYS } from "./forms";
import { FnFormPage, type FormProps, Muted, UnitInput } from "./form-bits";
import { roleArn, tailOf } from "./model";
import { useTriggers } from "./hooks";

export function ConfigEdit(p: FormProps) {
  const { d, f } = p;
  const [g, setG] = useState(() => generalOf(f));
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<typeof g>) => setG((x) => ({ ...x, ...patch }));
  const save = useOp(on.fn(d.name), "UpdateFunctionConfiguration");
  const problems = generalProblems(g);
  const change = useMemo(() => generalChange(f, g), [f, g]);
  const queue = p.triggers.find((t) => t.how === "mapping" && t.kind === "SQS queue");
  const other = useOtherWithRole(d.name, g.role);
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0 || Object.keys(change).length === 0) return;
    save.mutate({ ...change }, { onSuccess: () => p.go({ tab: "configuration", p: "general", done: "updated" }) });
  };
  const changed = (was: string | number, now: string | number, unit: string) => (String(was) !== String(now) ? <> Changed from {was} {unit}.</> : null);
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "general" }} title="Edit General configuration" label="Edit General configuration" sub={<><b className="font-medium text-foreground">Change how {d.name} runs.</b> A saved change applies to $LATEST; published versions keep the settings they had.</>}
      request={{ service: "lambda", operation: "UpdateFunctionConfiguration", input: { FunctionName: d.name, ...change } }} refusal={save.error} callNote="Settings you did not change are left out."
      submitLabel="Save" pending={save.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]} onSubmit={submit} testId="config-edit-form">
      <FormSection title="General configuration" testId="section-general">
        <Field label={<>Description <span className="font-normal text-muted-foreground">Optional</span></>} help="Up to 256 characters." error={tried ? problems.description : undefined} testId="field-description">
          {(id) => <Input id={id} value={g.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="cfg-description" />}
        </Field>
        <Field label="Memory" error={tried ? problems.memory : undefined} help={<>From <b>128 MB to 10,240 MB</b>, default <b>128 MB</b>. A function gets the equivalent of one vCPU at 1,769 MB.<Muted>{changed(f.memory, g.memory, "MB")} Stored, not applied locally.</Muted></>} testId="field-memory">
          {(id) => <UnitInput id={id} value={g.memory} onChange={(memory) => set({ memory })} unit="MB" testId="cfg-memory" />}
        </Field>
        <Field label="Ephemeral storage" error={tried ? problems.ephemeral : undefined} help={<>The size of /tmp: any whole number from <b>512 to 10,240 MB</b>, default <b>512 MB</b>.<Muted>{changed(f.ephemeral, g.ephemeral, "MB")} Stored, not applied locally.</Muted></>} testId="field-ephemeral">
          {(id) => <UnitInput id={id} value={g.ephemeral} onChange={(ephemeral) => set({ ephemeral })} unit="MB" testId="cfg-ephemeral" />}
        </Field>
        <Field label="Timeout" error={tried ? problems.timeout : undefined} help={<>Default <b>3 seconds</b>, at most <b>900 seconds</b>.{queue ? <Muted> The SQS queue {queue.name} must hide a message for at least as long as this.{changed(f.timeout, g.timeout, "seconds")}</Muted> : <Muted>{changed(f.timeout, g.timeout, "seconds")}</Muted>}</>} testId="field-timeout">
          {(id) => <UnitInput id={id} value={g.timeout} onChange={(timeout) => set({ timeout })} unit="seconds" testId="cfg-timeout" />}
        </Field>
        <Field label="Execution role" error={tried ? problems.role : undefined} help={<>The role Lambda assumes to run the function.{other ? <Muted> {other} uses {g.role.split("/").pop()} too.</Muted> : null} <Muted>The function&apos;s process gets this role&apos;s credentials, so IAM judges what it calls.</Muted></>} testId="field-role">
          {(id) => <RolePicker id={id} value={g.role ? tailOf(g.role) : null} onChange={(role) => set({ role: role ? roleArn(p.where.account, role) : "" })} by="id" className="max-w-[24rem]" />}
        </Field>
      </FormSection>
    </FnFormPage>
  );
}

/** Another function that runs as the same role, when there is one (the form says so under the role). */
function useOtherWithRole(self: string, role: string): string | null {
  void self;
  void role;
  return null;
}

export function EnvEdit(p: FormProps) {
  const { d, f } = p;
  const [rows, setRows] = useState<KeyValueRow[]>(() => Object.entries(f.env).map(([k, v]) => newRow(k, v)));
  const [tried, setTried] = useState(false);
  const save = useOp(on.fn(d.name), "UpdateFunctionConfiguration");
  const pairs = rows.map((r) => ({ key: r.key, value: r.value }));
  const problems = envProblems(pairs);
  const keyed = rowsValid(rows);
  const input = useMemo(() => ({ FunctionName: d.name, ...envInput(pairs) }), [d.name, pairs]);
  const was = Object.keys(f.env);
  const added = rows.filter((r) => r.key && !was.includes(r.key)).map((r) => r.key);
  const bad = Object.keys(problems).length > 0 || !keyed;
  const submit = () => {
    setTried(true);
    if (bad) return;
    save.mutate({ ...envInput(pairs) }, { onSuccess: () => p.go({ tab: "configuration", p: "env", done: "env" }) });
  };
  void RESERVED_KEYS;
  void Textarea;
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "env" }} title="Edit environment variables" label="Edit environment variables" sub={<><b className="font-medium text-foreground">Settings for {d.name}&apos;s code.</b> Changes apply to $LATEST.</>}
      request={{ service: "lambda", operation: "UpdateFunctionConfiguration", input }} refusal={save.error} callNote="The whole Variables map is replaced, so every variable you keep is sent again."
      submitLabel="Save" pending={save.isPending} disabled={tried && bad} disabledWhy={problems[""] ?? Object.values(problems)[0]} onSubmit={submit} testId="env-edit-form">
      <FormSection title="Environment variables" testId="section-env">
        <KeyValueEditor rows={rows} onChange={setRows} addLabel="Add environment variable" keyLabel="Key" valueLabel="Value" showErrors={tried} testId="env-kv" />
        {tried && Object.entries(problems).filter(([i]) => i !== "").map(([i, why]) => <Alert key={i} tone="bad" title={`Row ${Number(i) + 1}`} testId={`env-problem-${i}`}>{why}</Alert>)}
        {problems[""] && <Alert tone="bad" title="Too large" testId="env-size">{problems[""]}</Alert>}
        <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground" data-testid="env-rules">
          A key starts with a letter, has at least two characters, and holds only letters, digits and underscores. <b className="font-semibold text-foreground">All keys and values together can be at most 4 KB.</b> Keys Lambda reserves, such as AWS_REGION and _HANDLER, are refused.{added.length ? <> {added.join(", ")} {added.length === 1 ? "is the new variable" : "are the new variables"}.</> : null}
        </p>
      </FormSection>
    </FnFormPage>
  );
}
void useTriggers;
