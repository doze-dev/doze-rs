// Create key, steps 1 and 2 (kms__key-create, key-create-labels): the kind of key and what it is for, the options fixed at creation,
// then the alias, description and tags.
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field, FormSection, RadioCards, RuleHelp, TAG_RULES } from "../../../kit";
import { RowChoice } from "../cfg-parts";
import { aliasProblem, choose, descriptionProblem, type KeyForm, specsFor, usagesFor } from "./create-model";

export type StepProps = { f: KeyForm; set: (patch: Partial<KeyForm>) => void; show: boolean };

export function StepConfigure({ f, set, others, region }: Omit<StepProps, "show"> & { others: string | null; region: string }) {
  const usages = usagesFor(f.kind);
  const specs = specsFor(f.kind, f.usage);
  return (
    <>
      <FormSection title="Key configuration" testId="section-config">
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Key type</span>
          <RadioCards
            name="kind"
            label="Key type"
            value={f.kind}
            onChange={(kind) => set(choose(f, { kind }))}
            options={[
              { value: "symmetric", title: "Symmetric", body: "256-bit AES-GCM keys that encrypt and decrypt data under 4 KB. The most common type: it protects other data keys and is used by AWS services that encrypt your data." },
              { value: "asymmetric", title: "Asymmetric", body: "RSA or elliptic curve key pairs in various sizes, for encryption and decryption or for sign and verify." },
            ]}
          />
          <RuleHelp>Choose <b>Symmetric</b> for HMAC keys as well. AWS services that encrypt your data on your behalf need a symmetric encryption key.</RuleHelp>
        </div>
        <div className="grid gap-3">
          <span className="text-[14px] leading-none font-medium text-bright">Key usage</span>
          <RowChoice name="usage" label="Key usage" value={f.usage} onChange={(usage) => set(choose(f, { usage }))} options={usages.map((u) => ({ value: u.value, title: u.label, body: u.value === "ENCRYPT_DECRYPT" && f.kind === "symmetric" ? "Key spec SYMMETRIC_DEFAULT." : u.body }))} />
          {specs.length > 1 && (
            <Field label="Key spec" testId="field-spec">
              {(id) => (
                <Select value={f.spec} onValueChange={(spec) => set(choose(f, { spec }))}>
                  <SelectTrigger id={id} className="w-full max-w-sm bg-card font-mono" data-testid="key-spec"><SelectValue /></SelectTrigger>
                  <SelectContent>{specs.map((s) => <SelectItem key={s} value={s} className="font-mono">{s}</SelectItem>)}</SelectContent>
                </Select>
              )}
            </Field>
          )}
          <RuleHelp>The key usage <b>cannot be changed</b> after the key is created.{others && <> Your other keys: {others}</>}</RuleHelp>
        </div>
      </FormSection>
      <FormSection title="Advanced options" collapsible defaultOpen description="Both settings are fixed at creation." testId="section-advanced">
        <div className="grid gap-3">
          <span className="text-[14px] leading-none font-medium text-bright">Key material origin</span>
          <RowChoice
            name="origin"
            label="Key material origin"
            value="kms"
            onChange={() => undefined}
            options={[
              { value: "kms", title: "KMS", body: "KMS generates the key material." },
              { value: "external", title: "External (Import key material)", off: "Not built in this form: create the key with Origin EXTERNAL from the AWS CLI, then import the material with ImportKeyMaterial." },
              { value: "hsm", title: "AWS CloudHSM key store", off: "Not available in doze-aws: custom key stores are refused, because there is no CloudHSM cluster to attach." },
              { value: "xks", title: "External key store", off: "Not available in doze-aws: custom key stores are refused, because there is no external key manager to attach." },
            ]}
          />
        </div>
        <div className="grid gap-3">
          <span className="text-[14px] leading-none font-medium text-bright">Regionality</span>
          <RowChoice
            name="region"
            label="Regionality"
            value={f.multiRegion ? "multi" : "single"}
            onChange={(v) => set({ multiRegion: v === "multi" })}
            options={[
              { value: "single", title: "Single-Region key", body: `The key exists only in ${region}.` },
              { value: "multi", title: "Multi-Region key", body: `A primary key with a mrk- id; replicas in other Regions are made afterwards with ReplicateKey.` },
            ]}
          />
        </div>
      </FormSection>
    </>
  );
}

export function StepLabels({ f, set, show, taken }: StepProps & { taken: string[] }) {
  const problem = aliasProblem(f.alias, taken);
  const long = descriptionProblem(f.description);
  return (
    <>
      <FormSection title="Alias and description" testId="section-alias">
        <Field label="Alias" error={show || f.alias !== "" ? (problem ?? undefined) : undefined} testId="field-alias" help={<><b>Do not type alias/</b>; the console adds it. It cannot begin with <b>aws/</b>, which is reserved for AWS managed keys, and it must be unique in this account and Region. 1 to 256 characters: letters, digits, / _ and -.{taken.length > 0 && <> Aliases in use: {taken.join(", ")}.</>}</>}>
          {(id) => <Input id={id} value={f.alias} onChange={(e) => set({ alias: e.target.value })} placeholder="larder-reports" autoComplete="off" spellCheck={false} className="max-w-xl bg-card font-mono" data-testid="key-alias" />}
        </Field>
        <Field label="Description" optional error={long ?? undefined} testId="field-description" help="Up to 8,192 characters. Use it to say which application or data the key protects; it can be changed later. Keep confidential information out of it: it may be displayed in plaintext in logs and other output.">
          {(id) => <Textarea id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} rows={3} className="bg-card" data-testid="key-description" />}
        </Field>
      </FormSection>
      <FormSection title="Tags" optional description="A tag is a key and an optional value. Tags can also control access to the key." testId="section-tags">
        <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={show} keyLabel="Tag key" valueLabel="Tag value" addLabel="Add tag" removeLabel="Remove" testId="key-tags" />
        <RuleHelp>Tag keys are 1 to 128 characters, values 0 to 256, and both are case-sensitive. A key can have up to 50 tags, and keys cannot begin with aws:. Changing a tag can allow or deny access to the key.</RuleHelp>
      </FormSection>
    </>
  );
}
