// The form of a parameter (ssm__parameter-create, parameter-create-exists, parameter-edit): the name, description, tier, type, data
// type, KMS key, value and tags, with the PutParameter call beside it. A create refuses a name that exists; an edit sends Overwrite
// and makes the next version, and cannot change the type.
import { Link, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { rowsValid } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { useAbout } from "../../../api/queries";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, Field, FormPage, FormSection, RadioCards, RuleHelp, TAG_RULES } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { fieldOf } from "../cfg-parts";
import { KeyAliasPicker } from "../key-picker";
import { crumbs } from "../kms/parts";
import { refusalOf } from "../kms/schedule";
import { blankParam, nameProblem, type ParamForm, paramRows, putInput, type Tier, valueProblem } from "./model";

export const PARAMETERS = { service: "ssm", kind: "parameters" } as const;

const TIERS = [
  { value: "Standard", title: "Standard", body: "The default tier. A value of up to 4 KB." },
  { value: "Advanced", title: "Advanced", body: "A value of up to 8 KB, and parameter policies on AWS. An Advanced parameter can never go back to Standard." },
] as const;
const TYPES = [
  { value: "String", title: "String", body: "Any block of text." },
  { value: "StringList", title: "StringList", body: "Comma-separated values." },
  { value: "SecureString", title: "SecureString", body: "Value encrypted at rest." },
] as const;

/** `d`: the parameter being edited; none for a create. */
export function ParameterForm({ d }: { d?: ResourceDetail }) {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const region = useAbout().data?.region ?? "us-east-1";
  const editing = d !== undefined;
  const list = useAllRows("ssm", "parameters");
  const known = useMemo(() => paramRows(list.rows, list.columns), [list.rows, list.columns]);
  const first = list.first;
  const [f, setF] = useState<ParamForm>(() => (d ? { ...blankParam(), name: d.name, description: fieldOf(d, "Description") ?? "", tier: (fieldOf(d, "Tier") === "Advanced" ? "Advanced" : "Standard") as Tier, type: (fieldOf(d, "Type") ?? "String") as ParamForm["type"], key: fieldOf(d, "Key") ?? null, value: fieldOf(d, "Type") === "SecureString" ? "" : (fieldOf(d, "Value") ?? "") } : blankParam()));
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  // A SecureString is edited from its own value: read decrypted when the form opens, until the person has typed.
  const typed = useRef(false);
  const secureEdit = editing && fieldOf(d, "Type") === "SecureString";
  const current = useQuery({ queryKey: ["resources", "kindop", "ssm", "parameters", "GetParameter", d?.name, "edit"], queryFn: ({ signal }) => callKindOp<{ Parameter: { Value: string } }>(PARAMETERS, "GetParameter", { Name: d?.name, WithDecryption: true }, signal), enabled: secureEdit, retry: false, staleTime: 0, gcTime: 0 });
  useEffect(() => {
    const read = current.data?.Parameter.Value;
    if (read !== undefined && !typed.current) setF((x) => ({ ...x, value: read }));
  }, [current.data]);
  const set = (patch: Partial<ParamForm>) => { if (patch.value !== undefined) typed.current = true; setF((x) => ({ ...x, ...patch })); setRefused(null); };
  const exists = !editing && known.some((p) => p.name === f.name);
  const existing = known.find((p) => p.name === f.name);
  const problems = { name: editing ? null : nameProblem(f.name), value: valueProblem(f.value, f.tier), tags: rowsValid(f.tags, TAG_RULES) ? null : "Fix the tags marked." };
  const bad = Object.values(problems).some(Boolean);
  const taken = refused?.code === "ParameterAlreadyExists";
  const input = putInput(f, { overwrite: editing, editing });
  const back = () => void navigate({ to: editing ? "/resources/$service/$kind/$name" : "/resources/$service/$second", params: editing ? { service: "ssm", kind: "parameters", name: d.name } : { service: "ssm", second: "parameters" }, search: {} } as never);
  const submit = async () => {
    setTried(true);
    if (bad) return;
    setPending(true);
    setRefused(null);
    try {
      const made = await callKindOp<{ Version: number }>(PARAMETERS, "PutParameter", input);
      await qc.invalidateQueries({ queryKey: ["resources"] });
      if (editing) void navigate({ to: "/resources/$service/$kind/$name", params: { service: "ssm", kind: "parameters", name: d.name }, search: { done: "saved", ...({ ver: String(made.Version) } as object) } } as never);
      else void navigate({ to: "/resources/$service/$second", params: { service: "ssm", second: "parameters" }, search: { do: "created", name: f.name, ver: String(made.Version), key: f.type } });
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const suggestion = `${f.name.replace(/\/[^/]*$/, "")}/${(f.name.split("/").pop() ?? "") || "value"}-2`;
  return (
    <FormPage
      title={editing ? "Edit" : "Create parameter"}
      mono={false}
      sub={editing ? <><span className="font-mono text-bright">{d.name}</span> is a {f.type} at version {fieldOf(d, "Version")}. Saving a new value makes version {Number(fieldOf(d, "Version") ?? 0) + 1} and keeps versions 1 to {fieldOf(d, "Version")}. The console can&apos;t change the type of a parameter that exists, for example String to SecureString.</> : `Parameters are only available in the Region where they were created (${region}).`}
      crumbs={editing ? [{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: "/resources/ssm" }, { label: "Parameters", to: "/resources/ssm/parameters" }, { label: d.name, to: `/resources/ssm/parameters/${encodeURIComponent(d.name)}` }, { label: "Edit", to: "" }] : [...crumbs(first?.group, first?.service_title).slice(0, -1), { label: "Parameter Store", to: "/resources/ssm" }, { label: "Create parameter", to: "" }]}
      request={{ service: "ssm", operation: "PutParameter", input }}
      showInput
      callTitle={taken ? "The call that was refused" : undefined}
      refusal={refused}
      callNote={taken ? `After you change the name, Create parameter makes the same call with the new name.` : editing ? "The type, the data type and the key are left out: they stay as the parameter has them. Overwrite is true, so the parameter that exists is updated, and the response carries the new version." : "Overwrite is not sent, so its default, false, applies: a name that exists is refused."}
      banner={refused ? <Alert tone="bad" title={taken ? `PutParameter was refused with ${refused.code} (HTTP 400).` : `The parameter was not ${editing ? "saved" : "created"}: ${refused.code}`} testId="put-refused">{taken ? `The error's description: ${refused.message} Nothing was created or changed.` : refused.message}</Alert> : undefined}
      submitLabel={editing ? "Save changes" : "Create parameter"}
      pending={pending}
      disabled={tried && bad}
      disabledWhy={Object.values(problems).find(Boolean)}
      onSubmit={() => void submit()}
      onCancel={back}
      testId="parameter-form"
    >
      <FormSection title="Parameter details" testId="section-details">
        <Field label="Name" error={tried || f.name !== "" ? (problems.name ?? undefined) : undefined} refusal={taken ? { why: `${f.name} already exists in ${region}.`, suggestion: <>Pick a new name, for example <span className="font-mono">{suggestion}</span>. To save a new value under this name instead, choose Edit on <Link to="/resources/$service/$kind/$name" params={{ service: "ssm", kind: "parameters", name: f.name }} search={{ view: "edit" }} className="font-mono text-link hover:underline">{f.name}</Link>: that overwrites it{existing ? ` as version ${Number(existing.version) + 1}` : ""}. Names are case sensitive.</> } : null} hint={!taken && exists ? <>{f.name} already exists in {region}. Creating it again is refused; choose Edit on it to save a new version.</> : undefined} help={editing ? undefined : <>Letters, digits and <b>. - _</b>, with / between levels, up to <b>15 levels</b>, and not starting with aws or ssm. Names are case sensitive, and on AWS the limit of 1,011 characters counts the 45 of <span className="font-mono">arn:aws:ssm:{region}:{"<account>"}:parameter/</span>.</>} testId="field-name">
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} readOnly={editing} placeholder="/larder/prod/db/password" autoComplete="off" spellCheck={false} className="max-w-xl bg-card font-mono" data-testid="parameter-name" />}
        </Field>
        <Field label="Description" optional error={f.description.length > 1024 ? "A description is at most 1,024 characters." : undefined} help="Up to 1,024 characters. Don't enter personally identifiable information.">
          {(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} className="max-w-xl bg-card" data-testid="parameter-description" />}
        </Field>
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Parameter tier</span>
          <RadioCards name="tier" label="Parameter tier" value={f.tier} onChange={(tier) => set({ tier })} options={TIERS} />
          <RuleHelp>On AWS, Standard holds values up to <b>4 KB</b> and Advanced up to <b>8 KB</b>, and an Advanced parameter can never go back to Standard. doze-aws holds to those sizes.</RuleHelp>
        </div>
        {!editing && <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Type</span>
          <RadioCards name="type" label="Type" value={f.type} onChange={(type) => set({ type })} disabled={editing} className="sm:grid-cols-3" options={TYPES} />
          <RuleHelp>Only the value of a SecureString is encrypted; the name, description and other properties are not. doze-aws seals the value with the KMS key below. The type can&apos;t be changed afterwards in the console.</RuleHelp>
        </div>}
        {!editing && <Field label="Data type" help={<>The default, <b>text</b>, checks nothing. Other data types make AWS validate the value (integer, boolean, json and more); only text is offered here.</>}>
          {(id) => <Input id={id} value="text" readOnly className="max-w-40 bg-muted font-mono" data-testid="parameter-datatype" />}
        </Field>}
        {!editing && <Field label="KMS Key ID" optional={f.type !== "SecureString"} help={<>A key id, key ARN, alias name or alias ARN; an alias is written with the <span className="font-mono">alias/</span> prefix. Left empty, AWS uses <span className="font-mono">alias/aws/ssm</span>. {f.type === "SecureString" ? "doze-aws seals the value with this key, through KMS, so the key must exist and be enabled." : "It applies to a SecureString only."}</>}>
          {(id) => <KeyAliasPicker id={id} value={f.type === "SecureString" ? f.key : null} onChange={(key) => set({ key })} defaultLabel="aws/ssm" />}
        </Field>}
        <Field label="Value" error={tried ? (problems.value ?? undefined) : undefined} help={<>On AWS a value is limited to <b>4 KB</b> on Standard and <b>8 KB</b> on Advanced, and can&apos;t contain <span className="font-mono">{"{{ }}"}</span> or <span className="font-mono">{"{{ssm:name}}"}</span>. This one is {new TextEncoder().encode(f.value).length.toLocaleString("en-US")} characters.</>} hint={secureEdit ? (current.isError ? "The current value could not be read. Saving makes a new version with what you enter." : "The current value is read decrypted when this form opens, so the call is on the record; saving makes a new version with what is here.") : undefined} testId="field-value">
          {(id) => <Textarea id={id} value={f.value} onChange={(e) => set({ value: e.target.value })} rows={4} spellCheck={false} className="bg-card font-mono text-[13px]" data-testid="parameter-value" />}
        </Field>
      </FormSection>
      {!editing && (
        <FormSection title="Tags" optional description="Optional metadata: key and value pairs that categorize a parameter." testId="section-tags">
          <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={tried} keyLabel="Key" valueLabel="Value" addLabel="Add tag" removeLabel="Remove" testId="parameter-tags" />
          <RuleHelp>A parameter can have up to 50 tags. Tags on an existing parameter are edited on its Tags tab.</RuleHelp>
        </FormSection>
      )}
    </FormPage>
  );
}

