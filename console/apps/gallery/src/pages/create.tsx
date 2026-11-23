import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, pairsOf, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { ReviewSection, type Step, Stepper } from "@doze/ui/components/app/stepper";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Switch } from "@doze/ui/components/ui/switch";
import { CallPanel, Field, FormPage, FormSection, LocalStatus, type PreviewRequest, RuleHelp, TAG_RULES, type Wizard } from "@aws-kit";
import { Section, Specimen } from "../specimen";

const CRUMBS = [{ label: "Resources", to: "/resources" }, { label: "Messaging and events", to: "/resources" }, { label: "SQS", to: "/resources" }];

/** The Create queue form of the SQS board: the name, a setting with its rule, optional sections that are shut with a summary, and
 * the call at the right. With `refused`, the name is taken and the service has said so. */
function QueueForm({ refused = false }: { refused?: boolean }) {
  const [name, setName] = useState(refused ? "pick-lists" : "");
  const [vis, setVis] = useState(refused ? "60" : "30");
  const [fifo, setFifo] = useState("standard");
  const [encrypt, setEncrypt] = useState(true);
  const [tags, setTags] = useState<KeyValueRow[]>([]);
  const [done, setDone] = useState(false);
  const [pending, setPending] = useState(false);
  const full = fifo === "fifo" && name && !name.endsWith(".fifo") ? `${name}.fifo` : name;
  const attrs: Record<string, string> = {};
  if (vis !== "30") attrs.VisibilityTimeout = vis;
  if (fifo === "fifo") attrs.FifoQueue = "true";
  const request: PreviewRequest = { service: "sqs", operation: "CreateQueue", input: { QueueName: full, ...(Object.keys(attrs).length ? { Attributes: attrs } : {}), ...(filled(tags).length ? { tags: Object.fromEntries(pairsOf(filled(tags)).map((t) => [t.key, t.value])) } : {}) } };
  const taken = refused && name === "pick-lists";
  const rule = /^[A-Za-z0-9_-]{1,80}(\.fifo)?$/.test(full);
  return (
    <FormPage
      title="Create queue"
      sub="The console provides default values for all settings except the queue name."
      crumbs={[...CRUMBS, { label: "Create queue", to: "/resources" }]}
      request={request}
      refusal={taken ? { code: "QueueNameExists", message: "A queue named pick-lists already exists with a different value for VisibilityTimeout." } : null}
      callNote={taken ? "The existing queue has VisibilityTimeout 30. Sending the exact same attributes as the existing queue would have returned its URL instead of an error." : "Only the settings that differ from the defaults are sent, so SQS applies its own defaults to the rest."}
      banner={
        done ? (
          <ResultBanner title={`Queue ${full} created.`} returned="CreateQueue returned its URL, and the queue's Details page opens." onDismiss={() => setDone(false)} />
        ) : taken ? (
          <Alert tone="bad" title="Queue not created.">
            QueueNameExists: a queue named pick-lists already exists with a different value for VisibilityTimeout. A request is refused only when an attribute it carries differs from the existing queue's.
          </Alert>
        ) : undefined
      }
      submitLabel="Create queue"
      pending={pending}
      disabled={!rule}
      disabledWhy="Give the queue a name: letters, digits, hyphens and underscores."
      onCancel={() => setName("")}
      onSubmit={() => {
        setPending(true);
        setTimeout(() => {
          setPending(false);
          setDone(true);
        }, 400);
      }}
    >
      <FormSection title="Type and name">
        <div className="grid gap-1.5">
          <Label>Type</Label>
          <RadioGroup value={fifo} onValueChange={setFifo} className="grid gap-3 sm:grid-cols-2" aria-label="Queue type">
            {[["standard", "Standard", "Maximum throughput, best-effort ordering."], ["fifo", "FIFO", "Strict message ordering and uniqueness. The name must end in .fifo."]].map(([v, t, d]) => (
              <Label key={v} htmlFor={`type-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem id={`type-${v}`} value={v!} className="mt-0.5" />
                <span className="grid gap-0.5">
                  <span className="font-medium text-bright">{t}</span>
                  <span className="text-[13px] text-muted-foreground">{d}</span>
                </span>
              </Label>
            ))}
          </RadioGroup>
          <RuleHelp>You can't change the type after you create the queue, and a standard queue can't be converted to FIFO.</RuleHelp>
        </div>
        <Field label="Name" help={<>Up to <b>80</b> characters: letters, digits, hyphens and underscores. Names are case-sensitive.</>} hint={name === "pick-lists" && !refused ? "pick-lists already exists in this Region." : undefined} refusal={taken ? { why: "A queue with this name already exists, with different attributes.", suggestion: "Open pick-lists, or use another name." } : null}>
          {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="pick-lists" autoComplete="off" spellCheck={false} className="font-mono" data-testid="queue-name" />}
        </Field>
      </FormSection>
      <FormSection title="Configuration" description="The console sets default values for the queue configuration parameters.">
        <Field label="Visibility timeout (seconds)" help={<>Range <b>0 seconds</b> to <b>12 hours</b>, default <b>30 seconds</b>.</>} hint={vis !== "30" ? "pick-lists uses 30 seconds." : undefined}>
          {(id) => <Input id={id} value={vis} onChange={(e) => setVis(e.target.value)} inputMode="numeric" className="w-40 font-mono" data-testid="queue-visibility" />}
        </Field>
      </FormSection>
      <FormSection title="Encryption" collapsible defaultOpen={false} summary={encrypt ? "Enabled with an Amazon SQS key (SSE-SQS)." : "Disabled."} description="The server-side encryption settings of the queue.">
        <div className="flex items-center gap-3">
          <Switch id="sse" checked={encrypt} onCheckedChange={setEncrypt} />
          <Label htmlFor="sse">{encrypt ? "Enabled" : "Disabled"}</Label>
        </div>
        <LocalStatus state="stored" reason="doze-aws keeps the encryption settings and returns them; it encrypts no message." />
      </FormSection>
      <FormSection title="Tags" optional collapsible defaultOpen={false} summary={filled(tags).length ? `${filled(tags).length} tags.` : "No tags."} description="Cost allocation tags. Tags are case-sensitive, and a new tag with an existing key overwrites it.">
        <KeyValueEditor rows={tags} onChange={setTags} rules={TAG_RULES} testId="form-tags" />
      </FormSection>
    </FormPage>
  );
}

const STEPS: Step[] = [{ id: "type", label: "Choose secret type" }, { id: "configure", label: "Configure secret" }, { id: "rotation", label: "Configure rotation", optional: true }, { id: "review", label: "Review" }];

/** The Store a new secret wizard: four steps, the review with an Edit link on each step's answers. */
function SecretWizard() {
  const [current, setCurrent] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [name, setName] = useState("larder/prod/search-key");
  const [type, setType] = useState("other");
  const [rotate, setRotate] = useState(false);
  const go = (i: number) => {
    setCurrent(i);
    setFurthest((f) => Math.max(f, i));
  };
  const wizard: Wizard = { steps: STEPS, current, furthest, onStep: go, onBack: () => go(Math.max(0, current - 1)), onNext: () => go(Math.min(STEPS.length - 1, current + 1)) };
  return (
    <FormPage
      title="Store a new secret"
      sub={`Step ${current + 1} of ${STEPS.length}. ${current === 3 ? "Check the details, then store the secret." : "Choose, then continue."}`}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Config and secrets", to: "/resources" }, { label: "Secrets Manager", to: "/resources" }]}
      request={{ service: "secretsmanager", operation: "CreateSecret", input: { Name: name, SecretString: "(masked)" } }}
      callNote="The answer carries the new secret's ARN, its name and the id of its first version, which gets the label AWSCURRENT."
      submitLabel="Store"
      onCancel={() => go(0)}
      onSubmit={() => undefined}
      wizard={wizard}
      testId="wizard"
    >
      {current === 0 && (
        <FormSection title="Choose secret type">
          <RadioGroup value={type} onValueChange={setType} aria-label="Secret type" className="grid gap-2">
            {[["other", "Other type of secret"], ["db", "Credentials for a database"], ["api", "API key"]].map(([v, t]) => (
              <Label key={v} className="flex cursor-pointer items-center gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem value={v!} /> {t}
              </Label>
            ))}
          </RadioGroup>
        </FormSection>
      )}
      {current === 1 && (
        <FormSection title="Configure secret">
          <Field label="Secret name" help={<>Up to <b>512</b> characters: letters, digits and <code>/_+=.@-</code>.</>}>
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} className="font-mono" />}
          </Field>
        </FormSection>
      )}
      {current === 2 && (
        <FormSection title="Configure rotation" optional>
          <div className="flex items-center gap-3">
            <Switch id="rot" checked={rotate} onCheckedChange={setRotate} />
            <Label htmlFor="rot">Automatic rotation</Label>
          </div>
          <LocalStatus state="inactive" reason="doze-aws stores the schedule and runs no rotation." />
        </FormSection>
      )}
      {current === 3 && (
        <>
          <ReviewSection title="Choose secret type" onEdit={() => go(0)} rows={[{ label: "Secret type", value: type === "other" ? "Other type of secret" : type }, { label: "Encryption key", value: <span className="font-mono">alias/larder-app</span> }]} />
          <ReviewSection title="Configure secret" onEdit={() => go(1)} rows={[{ label: "Secret name", value: <span className="font-mono">{name}</span> }, { label: "Description", value: "Search provider API key." }, { label: "Tags", value: <span className="flex flex-wrap gap-2">{["app: search-indexer", "env: prod"].map((t) => <span key={t} className="rounded-full border px-2.5 py-0.5 font-mono text-[12px]">{t}</span>)}</span> }]} />
          <ReviewSection title="Configure rotation" onEdit={() => go(2)} rows={[{ label: "Automatic rotation", value: rotate ? "On" : "Off" }]} />
        </>
      )}
    </FormPage>
  );
}

function StepperDemo() {
  const [current, setCurrent] = useState(2);
  const [furthest, setFurthest] = useState(2);
  return (
    <div className="grid w-full gap-3" data-testid="stepper-demo">
      <Stepper steps={STEPS} current={current} furthest={furthest} onStep={setCurrent} />
      <div className="flex gap-2">
        <button type="button" className="rounded-md border bg-card px-3 py-1 text-[13px]" onClick={() => { setCurrent((c) => Math.min(3, c + 1)); setFurthest((f) => Math.max(f, current + 1)); }} data-testid="stepper-next">Next</button>
        <button type="button" className="rounded-md border bg-card px-3 py-1 text-[13px]" onClick={() => setCurrent((c) => Math.max(0, c - 1))} data-testid="stepper-back">Back</button>
      </div>
    </div>
  );
}

const PREVIEW: PreviewRequest = { service: "sqs", operation: "CreateQueue", input: { QueueName: "pick-lists", Attributes: { VisibilityTimeout: "60" } } };
const OUTLINE: PreviewRequest = { service: "sns", operation: "CreateTopic", input: { Name: "deliveries-updated", Attributes: { DisplayName: "larder deliveries", KmsMasterKeyId: "alias/larder-app" }, Tags: [{ Key: "stack", Value: "larder" }, { Key: "app", Value: "fulfilment" }] } };
const LONG: PreviewRequest = { service: "lambda", operation: "CreateFunction", input: { FunctionName: "a-function-whose-name-is-long-enough-to-need-wrapping-in-any-column", Role: "arn:aws:iam::123456789012:role/a-role-with-a-very-long-name-that-has-no-break-in-it-at-all-whatsoever", Environment: { Variables: { QUEUE_URL: "https://sqs.us-east-1.amazonaws.com/123456789012/stock-moves-with-a-long-name", NOTE: "a sentence that is long enough to wrap onto a second line and then a third" } } } };

function LivePanel() {
  const [name, setName] = useState("pick-lists");
  return (
    <div className="grid w-full gap-3 md:grid-cols-2">
      <div className="grid content-start gap-1.5">
        <Label htmlFor="live-name">Queue name</Label>
        <Input id="live-name" value={name} onChange={(e) => setName(e.target.value)} className="font-mono" data-testid="live-name" />
        <p className="text-[12.5px] text-muted-foreground">Type: the panel follows once you pause.</p>
      </div>
      <CallPanel request={{ service: "sqs", operation: "CreateQueue", input: name ? { QueueName: name } : {} }} note="Only the settings that differ from the defaults are sent." testId="live-panel" />
    </div>
  );
}

export function CreatePage() {
  return (
    <>
      <PageHeader title="Create" sub="Full-page create flows: the sections of a form with their rules, the call it will make shown beside it as it is typed, and a wizard whose last step reviews every answer." />

      <Section id="stepper" title="Stepper" source="components/app/stepper.tsx" note="A step that was passed is a button back to it; the current one says so; one not reached is a label. After going back, the steps already reached can be walked forward again.">
        <Specimen label="Step 1 of 4" stack>
          <Stepper steps={STEPS} current={0} />
        </Specimen>
        <Specimen label="Going back and forth" stack>
          <StepperDemo />
        </Specimen>
        <Specimen label="Long labels" stack>
          <Stepper steps={[{ id: "a", label: "Choose the kind of secret you want to store in this account" }, { id: "b", label: "Configure the secret, its name and its description and its tags" }, { id: "c", label: "Review" }]} current={1} furthest={1} onStep={() => undefined} className="w-full" />
        </Specimen>
      </Section>

      <Section id="callpanel" title="CallPanel" source="apps/aws/src/kit/call-panel.tsx" note="The call a form will make, from POST /preview, drawn as the boards draw it: the operation over its input as JSON (the SNS boards as an outline), with the AWS CLI line and the boto3 snippet to copy and the JSON that is sent. It wraps at any width and never scrolls. It is asked for once the typing pauses, and the last answer stays while the next is on its way.">
        <Specimen label="Live" stack>
          <LivePanel />
        </Specimen>
        <Specimen label="As an outline (the SNS boards)" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={OUTLINE} format="outline" note="Settings left at their defaults are not sent." testId="panel-outline" />
          </div>
        </Specimen>
        <Specimen label="Long values wrap" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={LONG} testId="panel-long" />
          </div>
        </Specimen>
        <Specimen label="Long values wrap, as an outline" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={LONG} format="outline" testId="panel-long-outline" />
          </div>
        </Specimen>
        <Specimen label="Needs a member" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={{ service: "sqs", operation: "CreateQueue", input: {} }} testId="panel-missing" />
          </div>
        </Specimen>
        <Specimen label="Refused" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={PREVIEW} refusal={{ code: "QueueNameExists", message: "A queue named pick-lists already exists with a different value for VisibilityTimeout." }} note="The existing queue has VisibilityTimeout 30." testId="panel-refused" />
          </div>
        </Specimen>
        <Specimen label="Nothing to send yet" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={null} testId="panel-idle" />
          </div>
        </Specimen>
        <Specimen label="Not built" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={{ service: "sqs", operation: "Unbuilt", input: {} }} testId="panel-unbuilt" />
          </div>
        </Specimen>
        <Specimen label="Loading" stack>
          <div className="w-full max-w-sm">
            <CallPanel request={{ service: "sqs", operation: "Hang", input: {} }} testId="panel-loading" />
          </div>
        </Specimen>
      </Section>

      <Section id="formpage" title="FormPage" source="apps/aws/src/kit/form-page.tsx" note="Sections, the rule of each field in the service's numbers, hints from what is really there, shut sections that still say what they hold, and the CallPanel docked at the right. The footer says why the button is off.">
        <Specimen label="Create queue" stack>
          <div className="w-full rounded-xl border bg-background p-6" data-testid="form-live">
            <QueueForm />
          </div>
        </Specimen>
        <Specimen label="Create queue, refused" stack>
          <div className="w-full rounded-xl border bg-background p-6" data-testid="form-refused">
            <QueueForm refused />
          </div>
        </Specimen>
      </Section>

      <Section id="wizard" title="FormPage with a wizard" source="apps/aws/src/kit/form-page.tsx" note="The Stepper above the title; Back and Next until the last step, whose button stores. The review lists each step's answers with an Edit link back to that step.">
        <Specimen label="Store a new secret" stack>
          <div className="w-full rounded-xl border bg-background p-6">
            <SecretWizard />
          </div>
        </Specimen>
      </Section>
    </>
  );
}
