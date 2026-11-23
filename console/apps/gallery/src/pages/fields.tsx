import { useState } from "react";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { rowsFrom } from "@doze/ui/components/app/key-values";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Input } from "@doze/ui/components/ui/input";
import { BusPicker, Field, FormSection, FunctionPicker, KeyPicker, LocalStatus, PolicyEditor, PolicyPicker, QueuePicker, ResourcePicker, RolePicker, RuleHelp, TAG_RULES, TopicPicker } from "@aws-kit";
import { Section, Specimen } from "../specimen";

const IDENTITY = JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "Send", Effect: "Allow", Action: ["sqs:SendMessage", "sqs:GetQueueUrl"], Resource: "arn:aws:sqs:us-east-1:123456789012:pick-lists" }, { Sid: "NoPurge", Effect: "Deny", Action: "sqs:PurgeQueue", Resource: "*", Condition: { Bool: { "aws:SecureTransport": "false" } } }] }, null, 2);
const RESOURCE = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "sns.amazonaws.com" }, Action: "sqs:SendMessage", Resource: "arn:aws:sqs:us-east-1:123456789012:pick-lists" }] }, null, 2);
const BROKEN = JSON.stringify({ Statement: [{ Effect: "Permit", Principal: "*" }] }, null, 2);

function Policy({ initial, kind, name }: { initial: string; kind?: "resource"; name: string }) {
  const [v, setV] = useState(initial);
  return <PolicyEditor value={v} onChange={setV} kind={kind} maxSize={6144} className="w-full" testId={`policy-${name}`} />;
}

function Pairs({ initial, show = false, max }: { initial: { key: string; value: string }[]; show?: boolean; max?: number }) {
  const [rows, setRows] = useState(() => rowsFrom(initial));
  return <KeyValueEditor rows={rows} onChange={setRows} rules={max ? { ...TAG_RULES, maxRows: max } : TAG_RULES} showErrors={show} className="w-full max-w-3xl" testId={`kv-${initial.length}-${show}-${max ?? 0}`} />;
}

function Picker({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid content-start gap-1.5">
      <span className="text-[13px] font-medium text-foreground">{label}</span>
      {children}
    </div>
  );
}

export function FieldsPage() {
  const [q, setQ] = useState<string | null>(null);
  return (
    <>
      <PageHeader title="Fields" sub="The pieces a form is made of that are particular to doze-aws: fields with the service's rules and a refusal marker, sections that stay readable when shut, key-value rows, policies, and the pickers of other resources." />

      <Section id="field" title="Field, RuleHelp and FieldRefusal" source="apps/aws/src/kit/form-parts.tsx" note="The rule is the service's own numbers; the hint is drawn from what is really there; a refusal is the service's own words on the field, with what would work.">
        <Specimen label="Rule and hint" className="max-w-xl">
          <Field label="Visibility timeout (seconds)" help={<>Range <b>0 seconds</b> to <b>12 hours</b>, default <b>30 seconds</b>.</>} hint="pick-lists uses 30 seconds.">
            {(id) => <Input id={id} defaultValue="60" className="w-40 font-mono" />}
          </Field>
        </Specimen>
        <Specimen label="Refused" className="max-w-xl">
          <Field label="Name" refusal={{ why: "A queue with this name already exists, with different attributes.", suggestion: "Open pick-lists, or use another name." }} help={<>Up to <b>80</b> characters.</>}>
            {(id) => <Input id={id} defaultValue="pick-lists" className="font-mono" data-testid="refused-input" />}
          </Field>
        </Specimen>
        <Specimen label="A problem the form found" className="max-w-xl">
          <Field label="Message retention (days)" error="Enter a whole number from 1 to 14.">
            {(id) => <Input id={id} defaultValue="30" className="w-40 font-mono" />}
          </Field>
        </Specimen>
        <Specimen label="RuleHelp alone" className="max-w-xl">
          <RuleHelp>You can't change the type after you create the queue. <b>Range</b> 1 KiB to 1,024 KiB.</RuleHelp>
        </Specimen>
      </Section>

      <Section id="section" title="FormSection and LocalStatus" source="apps/aws/src/kit/form-parts.tsx" note="A section that is shut says what it holds. LocalStatus uses the same two phrases wherever doze-aws keeps a setting it does not act on.">
        <Specimen label="Open" stack>
          <FormSection title="Dead-letter queue" optional collapsible description="Receives messages that could not be processed from this queue." className="w-full" testId="sec-open">
            <Field label="Queue">{() => <span className="text-[13.5px] text-muted-foreground">pick-lists-dlq</span>}</Field>
          </FormSection>
        </Specimen>
        <Specimen label="Shut, with a summary" stack>
          <FormSection title="Encryption" collapsible defaultOpen={false} summary="Enabled with an Amazon SQS key (SSE-SQS)." className="w-full" testId="sec-shut">
            <LocalStatus state="stored" reason="doze-aws keeps the encryption settings and returns them; it encrypts no message." />
          </FormSection>
        </Specimen>
        <Specimen label="LocalStatus, stored">
          <LocalStatus state="stored" reason="doze-aws keeps the encryption settings and returns them; it encrypts no message." className="max-w-xl" />
        </Specimen>
        <Specimen label="LocalStatus, not active">
          <LocalStatus state="inactive" reason="A trigger is stored; nothing invokes the function." className="max-w-xl" />
        </Specimen>
      </Section>

      <Section id="kv" title="KeyValueEditor" source="components/app/key-value-editor.tsx" note="Add and remove rows; an empty key and a repeated key are said under the row once the person has left the field (or tried to save). Tags are case-sensitive: Env and env are two keys.">
        <Specimen label="Filled" stack>
          <Pairs initial={[{ key: "app", value: "fulfilment" }, { key: "env", value: "dev" }]} />
        </Specimen>
        <Specimen label="Problems shown" stack>
          <Pairs show initial={[{ key: "env", value: "dev" }, { key: "env", value: "prod" }, { key: "", value: "orphan" }]} />
        </Specimen>
        <Specimen label="At its limit" stack>
          <Pairs max={2} initial={[{ key: "a", value: "1" }, { key: "b", value: "2" }]} />
        </Specimen>
        <Specimen label="Empty" stack>
          <Pairs initial={[]} />
        </Specimen>
        <Specimen label="Long text" stack>
          <Pairs initial={[{ key: "a-key-that-is-a-good-deal-longer-than-the-column-it-sits-in-to-see-what-the-input-does", value: "and a value to match: " + "v".repeat(120) }]} />
        </Specimen>
      </Section>

      <Section id="policy" title="PolicyEditor" source="apps/aws/src/kit/policy-editor.tsx" note="The JSON editor, the problems with the policy's shape, and what it says as sentences. A resource policy needs a Principal in each statement; one attached to a user, group or role must have none.">
        <Specimen label="Identity policy" stack>
          <Policy name="identity" initial={IDENTITY} />
        </Specimen>
        <Specimen label="Resource policy" stack>
          <Policy name="resource" initial={RESOURCE} kind="resource" />
        </Specimen>
        <Specimen label="Problems" stack>
          <Policy name="broken" initial={BROKEN} />
        </Specimen>
        <Specimen label="Not JSON" stack>
          <Policy name="notjson" initial={'{ "Version": '} />
        </Specimen>
      </Section>

      <Section id="pickers" title="Pickers" source="apps/aws/src/kit/pickers.tsx" note="One resource of another kind, searched from the list the console reads for it. Each says so while it reads, when it could not, and when there is nothing to choose.">
        <Specimen label="Seven kinds" stack>
          <div className="grid w-full gap-4 md:grid-cols-2 xl:grid-cols-3">
            <Picker label="Queue"><QueuePicker value={q} onChange={setQ} /></Picker>
            <Picker label="Topic"><TopicPicker value={null} onChange={() => undefined} /></Picker>
            <Picker label="Function"><FunctionPicker value={null} onChange={() => undefined} /></Picker>
            <Picker label="Role"><RolePicker value={null} onChange={() => undefined} /></Picker>
            <Picker label="Key"><KeyPicker value={null} onChange={() => undefined} /></Picker>
            <Picker label="Event bus"><BusPicker value="larder" onChange={() => undefined} /></Picker>
            <Picker label="Policy"><PolicyPicker value={null} onChange={() => undefined} /></Picker>
          </div>
        </Specimen>
        <Specimen label="Nothing to choose, and a failure" stack>
          <div className="grid w-full gap-4 md:grid-cols-2">
            <Picker label="No queues"><ResourcePicker service="sqs" kind="empty" noun="queue" aria-label="Queues, none" value={null} onChange={() => undefined} /></Picker>
            <Picker label="A failed read"><ResourcePicker service="sqs" kind="broken" noun="queue" aria-label="Queues, failed" value={null} onChange={() => undefined} /></Picker>
          </div>
        </Specimen>
      </Section>
    </>
  );
}
