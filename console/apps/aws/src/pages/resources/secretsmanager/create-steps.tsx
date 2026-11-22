// Store a new secret, steps 1 to 3 (secretsmanager__create-type, create-configure, create-rotation): what kind of secret and its value,
// its name, description, tags and permissions, and whether it rotates.
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { Callout } from "@doze/ui/components/app/callout";
import { PlusIcon } from "lucide-react";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { useAbout } from "../../../api/queries";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field, FormSection, FunctionPicker, PolicyEditor, RadioCards, RuleHelp, Segmented, TAG_RULES } from "../../../kit";
import { KeyAliasPicker } from "../key-picker";
import { durationProblem, MAX_VALUE, nameProblem, replicaProblem, replicaRegions, scheduleProblem, type SecretForm, valueProblem, secretValue } from "./model";

/** Regions offered as the person types one; any Region name is taken. */
const REGIONS = ["us-east-1", "us-east-2", "us-west-1", "us-west-2", "eu-west-1", "eu-west-2", "eu-west-3", "eu-central-1", "eu-north-1", "ap-south-1", "ap-southeast-1", "ap-southeast-2", "ap-northeast-1", "ap-northeast-2", "ca-central-1", "sa-east-1"];

export type StepProps = { f: SecretForm; set: (patch: Partial<SecretForm>) => void; show: boolean };
const OFF = (what: string) => `Not available in doze-aws: there is no ${what} to choose.`;
const PAIRS = { caseSensitive: true, maxKey: 256, emptyValue: true };

export function StepType({ f, set, show }: StepProps) {
  const problem = f.mode === "text" ? valueProblem(f.text) : valueProblem(f.pairs.some((p) => p.key !== "") ? secretValue(f) : "");
  return (
    <>
      <FormSection title="Secret type" description="Choose the type of credentials to store." testId="section-type">
        <RadioCards
          name="type"
          label="Secret type"
          value="other"
          onChange={() => undefined}
          className="sm:grid-cols-3"
          options={[
            { value: "rds", title: "Amazon RDS database", body: OFF("Amazon RDS database"), disabled: true },
            { value: "docdb", title: "Amazon DocumentDB database", body: OFF("Amazon DocumentDB database"), disabled: true },
            { value: "redshift", title: "Amazon Redshift data warehouse", body: OFF("Amazon Redshift data warehouse"), disabled: true },
            { value: "other", title: "Other type of secret", body: "API keys, access tokens, credentials that aren't for databases." },
            { value: "partner", title: "Partner secret", body: "Not available in doze-aws: it needs a third-party partner to hold and rotate the secret.", disabled: true },
          ]}
        />
      </FormSection>
      <FormSection title={f.mode === "pairs" ? "Key/value pairs" : "Plaintext"} description={`Enter the secret as JSON key/value pairs, or switch to Plaintext for any other format. You can store up to ${MAX_VALUE.toLocaleString("en-US")} bytes.`} actions={<Segmented label="Format" value={f.mode} onChange={(mode) => set({ mode, ...(mode === "text" && f.text === "" ? { text: secretValue(f) === "{}" ? "" : secretValue(f) } : {}) })} options={[{ value: "pairs", label: "Key/value" }, { value: "text", label: "Plaintext" }]} testId="value-mode" />} testId="section-value">
        {f.mode === "pairs" ? (
          <KeyValueEditor rows={f.pairs} onChange={(pairs) => set({ pairs })} rules={PAIRS} showErrors={show} keyLabel="Key" valueLabel="Value" addLabel="Add row" removeLabel="Remove" testId="value-rows" />
        ) : (
          <Field label="Plaintext" error={show ? (problem ?? undefined) : undefined}>{(id) => <Textarea id={id} value={f.text} onChange={(e) => set({ text: e.target.value })} rows={6} spellCheck={false} className="bg-card font-mono text-[13px]" data-testid="value-text" />}</Field>
        )}
        {show && f.mode === "pairs" && problem && <p className="text-[13px] text-bad" role="alert" data-testid="value-problem">{problem}</p>}
        <RuleHelp>The console stores the pairs as one JSON string, which is the shape a Lambda rotation function can parse.</RuleHelp>
      </FormSection>
      <FormSection title="Encryption key" testId="section-key">
        <Field label="Encryption key" help={<>Without a choice, Secrets Manager uses <span className="font-mono">aws/secretsmanager</span>. doze-aws encrypts every version with the key you pick, through KMS, so the key must exist and be enabled to store or read the secret.</>}>
          {(id) => <KeyAliasPicker id={id} value={f.key} onChange={(key) => set({ key })} defaultLabel="aws/secretsmanager" />}
        </Field>
      </FormSection>
    </>
  );
}

export function StepConfigure({ f, set, show, taken, openPolicy, setOpenPolicy }: StepProps & { taken: string[]; openPolicy: boolean; setOpenPolicy: (o: boolean) => void }) {
  const problem = nameProblem(f.name, taken);
  const region = useAbout().data?.region ?? "us-east-1";
  const hasPolicy = f.policy.trim() !== "";
  return (
    <>
      <FormSection title="Secret name and description" testId="section-name">
        <Field label="Secret name" error={show || f.name !== "" ? (problem ?? undefined) : undefined} help={<><b>1 to 512</b> characters: letters, digits and /_+=.@-. Don&apos;t end the name with a hyphen and six characters, because Secrets Manager adds a hyphen and six random characters to the ARN and a partial-ARN search could confuse the two.{taken.length > 0 && <> Existing secrets: {taken.slice(0, 4).join(", ")}{taken.length > 4 ? ` and ${taken.length - 4} more` : ""}.</>}</>} testId="field-name">
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="larder/prod/search-key" autoComplete="off" spellCheck={false} className="max-w-xl bg-card font-mono" data-testid="secret-name" />}
        </Field>
        <Field label="Description" optional error={f.description.length > 2048 ? "A description is at most 2,048 characters." : undefined} help="Up to 2,048 characters.">
          {(id) => <Textarea id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} rows={2} className="bg-card" data-testid="secret-description" />}
        </Field>
      </FormSection>
      <FormSection title="Tags" optional description="Tags are not encrypted, so keep sensitive information out of them." testId="section-tags">
        <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={show} keyLabel="Key" valueLabel="Value" addLabel="Add tag" removeLabel="Remove" testId="secret-tags" />
        <RuleHelp>Up to 50 tags per secret. Keys are 1 to 128 characters, unique and case-sensitive, and can&apos;t start with the reserved prefix aws:. Values are 0 to 256 characters.</RuleHelp>
      </FormSection>
      <FormSection title="Resource permissions" optional collapsible defaultOpen={openPolicy || hasPolicy} summary={hasPolicy ? "A resource policy will be attached." : "No resource policy is attached. You can add one now or from the secret's Overview tab."} description="Attach a resource-based policy that says who may use the secret." testId="section-permissions">
        <PolicyEditor kind="resource" value={f.policy} onChange={(policy) => { set({ policy }); setOpenPolicy(true); }} label="Resource policy" rows={10} maxSize={20480} help="Up to 20,480 characters. It is attached with PutResourcePolicy once the secret is stored." testId="policy-editor" />
      </FormSection>
      <FormSection title="Replicate secret" optional collapsible defaultOpen={f.replicas.length > 0} summary={replicaRegions(f.replicas).length > 0 ? `Replicated to ${replicaRegions(f.replicas).join(", ")}.` : "No replicas."} description="Copy the secret to other Regions." testId="section-replicate">
        <div className="grid gap-3" data-testid="replica-rows">
          {f.replicas.map((r, i) => {
            const bad = replicaProblem(r, i, f.replicas, region);
            return (
              <div key={i} className="grid gap-1">
                <div className="flex items-center gap-2">
                  <Input value={r} onChange={(e) => set({ replicas: f.replicas.map((x, j) => (j === i ? e.target.value : x)) })} list="replica-regions" aria-label={`Replica Region ${i + 1}`} aria-invalid={bad !== null} placeholder="eu-west-1" autoComplete="off" spellCheck={false} className="max-w-64 bg-card font-mono" data-testid="replica-region" />
                  <Button type="button" variant="outline" className="bg-card" onClick={() => set({ replicas: f.replicas.filter((_, j) => j !== i) })} data-testid="replica-remove">Remove</Button>
                </div>
                {bad && show && <p className="text-[13px] text-bad" role="alert">{bad}</p>}
              </div>
            );
          })}
          <datalist id="replica-regions">{REGIONS.filter((x) => x !== region).map((x) => <option key={x} value={x} />)}</datalist>
          <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ replicas: [...f.replicas, ""] })} data-testid="replica-add"><PlusIcon /> Add Region</Button></div>
        </div>
        <RuleHelp>Each Region gets a read-only copy of the secret with the same name, sealed under that Region&apos;s aws/secretsmanager key. Every change is made on this secret and reaches the copies; a copy of the secret&apos;s name that already exists there is refused.</RuleHelp>
      </FormSection>
    </>
  );
}

export function StepRotation({ f, set, show, lambdaServed }: StepProps & { lambdaServed: boolean }) {
  const sp = f.rotate ? scheduleProblem(f.schedule) : null;
  const dp = f.rotate ? durationProblem(f.duration) : null;
  return (
    <>
      <FormSection title="Automatic rotation" description="Secrets Manager rotates a secret by calling a Lambda function that you provide." testId="section-rotation">
        <div className="flex items-center gap-3"><Switch id="rotate" checked={f.rotate} onCheckedChange={(rotate) => set({ rotate })} data-testid="rotate-on" /><Label htmlFor="rotate" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">{f.rotate ? "Automatic rotation is on" : "Automatic rotation is off"}</span>{!f.rotate && <span className="text-[13px] text-muted-foreground">Turn it on from the Rotation tab of the secret whenever you are ready.</span>}</Label></div>
        {f.rotate && (
          <>
            <Field label="Rotation function" error={show && !f.fn ? "Choose the Lambda function that rotates this secret." : undefined} help="doze-aws invokes this Lambda function for each of the four rotation steps.">
              {(id) => (lambdaServed ? <FunctionPicker id={id} value={f.fn} onChange={(fn) => set({ fn })} by="name" /> : <p className="text-[13px] text-muted-foreground">Lambda is not running in this doze-aws, so there is no function to choose.</p>)}
            </Field>
            <Field label="Schedule expression" error={show ? (sp ?? undefined) : undefined} help={<>UTC, written as rate() or cron(), for example <span className="font-mono">rate(30 days)</span>. doze-aws starts a rotation when the schedule opens its window.</>}>
              {(id) => <Input id={id} value={f.schedule} onChange={(e) => set({ schedule: e.target.value })} className="max-w-xl bg-card font-mono" data-testid="rotation-schedule" />}
            </Field>
            <Field label="Window duration" optional error={show ? (dp ?? undefined) : undefined} help="The length of the rotation window in hours, for example 3h. Stored, not applied locally: the rotation starts when the window opens.">
              {(id) => <Input id={id} value={f.duration} onChange={(e) => set({ duration: e.target.value })} placeholder="3h" className="max-w-40 bg-card font-mono" data-testid="rotation-duration" />}
            </Field>
            <div className="flex items-start gap-3"><Checkbox id="now" checked={f.now} onCheckedChange={(v) => set({ now: v === true })} className="mt-0.5" data-testid="rotate-immediately" /><Label htmlFor="now" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Rotate immediately when the secret is stored</span><span className="text-[13px] text-muted-foreground">Choose it to rotate the secret as soon as it is stored.</span></Label></div>
          </>
        )}
      </FormSection>
      <Callout tone="note" lead="Rotation needs a Lambda function" testId="rotation-note">that knows how to change the credentials in the service this secret is for. Secrets Manager invokes it in four steps: createSecret, setSecret, testSecret and finishSecret. A schedule is applied: doze-aws starts the rotation when the schedule opens its window.</Callout>
    </>
  );
}
