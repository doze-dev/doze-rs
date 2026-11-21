// The fields of Create queue and Edit queue (sqs__queue-create, queue-create-fifo, queue-edit): the type and name, and the
// configuration with each setting's rule in SQS's own numbers. The optional sections are in ./form-sections.tsx.
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Field, FormSection, LocalStatus, RuleHelp } from "../../../kit";
import { LIMITS, type Problems, type QueueForm, type Time } from "./form-model";
import type { Unit } from "./queue-model";

export type FieldProps = { f: QueueForm; set: (patch: Partial<QueueForm>) => void; p: Problems; /** Problems are said once the person has tried to save (or left the field). */ show: boolean };

/** What the form says of the queues that are already there, drawn from them: `pick-lists uses 30 seconds.` */
export type Hints = { name?: React.ReactNode; visibility?: React.ReactNode; retention?: React.ReactNode; delay?: React.ReactNode };

const UNITS: Record<string, Unit[]> = { visibility: ["seconds", "minutes", "hours"], retention: ["seconds", "minutes", "hours", "days"], delay: ["seconds", "minutes"], wait: ["seconds"], reuse: ["seconds", "minutes", "hours"] };

/** A number and the unit it is in (the board's `seconds`, `days` select). */
export function TimeInput({ id, value, onChange, units, label, testId }: { id: string; value: Time; onChange: (t: Time) => void; units: Unit[]; label: string; testId: string }) {
  return (
    <div className="flex items-center gap-2">
      <Input id={id} value={value.value} onChange={(e) => onChange({ ...value, value: e.target.value })} inputMode="numeric" className="w-32 font-mono" autoComplete="off" data-testid={testId} />
      {units.length > 1 ? (
        <Select value={value.unit} onValueChange={(u) => onChange({ ...value, unit: u as Unit })}>
          <SelectTrigger aria-label={`${label} unit`} className="w-32 bg-card" data-testid={`${testId}-unit`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {units.map((u) => (
              <SelectItem key={u} value={u}>{u}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <span className="text-[13.5px] text-muted-foreground">{units[0]}</span>
      )}
    </div>
  );
}

export const timeUnits = (key: keyof typeof UNITS): Unit[] => UNITS[key]!;

/** A setting: the label, its input, the rule (SQS's numbers), and what the other queues say of it. */
function Setting({ label, name, rule, hint, problem, children, testId }: { label: string; name: string; rule: React.ReactNode; hint?: React.ReactNode; problem?: string; children: (id: string) => React.ReactNode; testId: string }) {
  return (
    <Field label={label} help={rule} hint={hint} error={problem} testId={testId} key={name}>
      {children}
    </Field>
  );
}

export function TypeAndName({ f, set, p, show, create, hints, refusal }: FieldProps & { create: boolean; hints?: Hints; /** The service said no to this name. */ refusal?: { why: React.ReactNode; suggestion?: React.ReactNode } | null }) {
  if (!create) {
    return (
      <FormSection title="Type and name" description="You can edit the queue's configuration parameters, except the queue type.">
        <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-y-2 text-[13.5px]">
          <dt className="text-muted-foreground">Type</dt>
          <dd className="text-bright" data-testid="edit-type">{f.fifo ? "FIFO" : "Standard"}</dd>
          <dt className="text-muted-foreground">Name</dt>
          <dd className="font-mono text-bright" data-testid="edit-name">{f.name}</dd>
        </dl>
      </FormSection>
    );
  }
  return (
    <FormSection title="Type and name">
      <div className="grid gap-1.5">
        <Label>Type</Label>
        <RadioGroup value={f.fifo ? "fifo" : "standard"} onValueChange={(v) => set({ fifo: v === "fifo" })} className="grid gap-3 sm:grid-cols-2" aria-label="Queue type">
          {[["standard", "Standard", "Maximum throughput, best-effort ordering."], ["fifo", "FIFO", "Strict message ordering and uniqueness. The name must end in .fifo."]].map(([v, t, d]) => (
            <Label key={v} htmlFor={`type-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
              <RadioGroupItem id={`type-${v}`} value={v!} className="mt-0.5" data-testid={`type-${v}`} />
              <span className="grid gap-0.5">
                <span className="font-medium text-bright">{t}</span>
                <span className="text-[13px] text-muted-foreground">{d}</span>
              </span>
            </Label>
          ))}
        </RadioGroup>
        <RuleHelp>You can't change the type after you create the queue{f.fifo ? "" : ", and a standard queue can't be converted to FIFO"}.</RuleHelp>
      </div>
      <Field label="Name" refusal={refusal} error={show ? p.name : undefined} help={f.fifo ? <>A FIFO queue name must end with <b>.fifo</b>, and the suffix counts toward the <b>80</b>-character limit.</> : <>Up to <b>80</b> characters: letters, digits, hyphens and underscores. Names are case-sensitive.</>} hint={hints?.name} testId="field-name">
        {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder={f.fifo ? "orders.fifo" : "pick-lists"} autoComplete="off" spellCheck={false} className="font-mono" data-testid="queue-name" />}
      </Field>
    </FormSection>
  );
}

export function Configuration({ f, set, p, show, create, hints }: FieldProps & { create: boolean; hints?: Hints }) {
  return (
    <FormSection title="Configuration" description={create ? "The console sets default values for the queue configuration parameters." : "Update the queue's configuration parameters."}>
      <div className="grid gap-x-8 gap-y-6 md:grid-cols-2">
      <Setting label="Visibility timeout" name="visibility" problem={show ? p.visibility : undefined} hint={hints?.visibility} testId="field-visibility" rule={<>How long a received message stays hidden from other consumers. Range <b>0 seconds</b> to <b>12 hours</b>, default <b>30 seconds</b>.</>}>
        {(id) => <TimeInput id={id} value={f.visibility} onChange={(visibility) => set({ visibility })} units={timeUnits("visibility")} label="Visibility timeout" testId="queue-visibility" />}
      </Setting>
      <Setting label="Message retention period" name="retention" problem={show ? p.retention : undefined} hint={hints?.retention} testId="field-retention" rule={<>How long SQS keeps a message nobody deletes. Range <b>1 minute</b> to <b>14 days</b>, default <b>4 days</b>.{create ? "" : " Lowering it below the age of existing messages can expire them."}</>}>
        {(id) => <TimeInput id={id} value={f.retention} onChange={(retention) => set({ retention })} units={timeUnits("retention")} label="Message retention period" testId="queue-retention" />}
      </Setting>
      <Setting label="Delivery delay" name="delay" problem={show ? p.delay : undefined} hint={hints?.delay ?? (f.fifo ? "FIFO queues take the delay at queue level only, not per message." : undefined)} testId="field-delay" rule={<>Hides every new message for this long after it is sent. Range <b>0 seconds</b> to <b>15 minutes</b>, default <b>0 seconds</b>.</>}>
        {(id) => <TimeInput id={id} value={f.delay} onChange={(delay) => set({ delay })} units={timeUnits("delay")} label="Delivery delay" testId="queue-delay" />}
      </Setting>
      <Setting label="Maximum message size" name="maxKib" problem={show ? p.maxKib : undefined} testId="field-max-size" rule={<>The largest message the queue accepts. Range <b>1 KiB</b> to <b>{LIMITS.maxKib.max.toLocaleString("en-US")} KiB</b> (1,048,576 bytes), default <b>1,024 KiB</b>.</>}>
        {(id) => (
          <div className="flex items-center gap-2">
            <Input id={id} value={f.maxKib} onChange={(e) => set({ maxKib: e.target.value })} inputMode="numeric" className="w-32 font-mono" autoComplete="off" data-testid="queue-max-size" />
            <span className="text-[13.5px] text-muted-foreground">KiB</span>
          </div>
        )}
      </Setting>
      <Setting label="Receive message wait time" name="wait" problem={show ? p.wait : undefined} testId="field-wait" rule={<>0 is short polling; any value above 0 is long polling. Range <b>0 to 20 seconds</b>, default <b>0 seconds</b>.{create ? " Long polling means fewer empty responses." : ""}</>}>
        {(id) => <TimeInput id={id} value={f.wait} onChange={(wait) => set({ wait })} units={timeUnits("wait")} label="Receive message wait time" testId="queue-wait" />}
      </Setting>
      </div>
      {f.fifo && <FifoSettings f={f} set={set} />}
    </FormSection>
  );
}

/** Content-based deduplication, high throughput FIFO, and the two attributes it sets. */
function FifoSettings({ f, set }: Pick<FieldProps, "f" | "set">) {
  return (
    <>
      <div className="flex items-start gap-3" data-testid="field-dedup">
        <Switch id="queue-dedup" checked={f.dedup} onCheckedChange={(dedup) => set({ dedup })} className="mt-0.5" data-testid="queue-dedup" />
        <div className="grid gap-1">
          <Label htmlFor="queue-dedup">Content-based deduplication {f.dedup ? "Enabled" : "Disabled"} <span className="rounded-full bg-muted px-2 py-px text-[11.5px] font-normal text-muted-foreground">Optional</span></Label>
          <RuleHelp>When on, SQS derives the deduplication ID from a SHA-256 hash of the message body, so a message with an identical body inside the 5-minute deduplication interval is accepted but not delivered. Default off.</RuleHelp>
        </div>
      </div>
      <div className="grid gap-3">
        <div className="flex items-start gap-3" data-testid="field-high-throughput">
          <Checkbox id="queue-high" checked={f.highThroughput} onCheckedChange={(on) => set({ highThroughput: on === true, ...(on === true ? { scope: "messageGroup", limit: "perMessageGroupId" } : { scope: "queue", limit: "perQueue" }) })} className="mt-0.5" data-testid="queue-high" />
          <div className="grid gap-1">
            <Label htmlFor="queue-high">Enable high throughput FIFO</Label>
            <RuleHelp>Sets <b>Deduplication scope</b> to messageGroup and <b>FIFO throughput limit</b> to perMessageGroupId. If you change either related option, normal throughput is in effect.</RuleHelp>
          </div>
        </div>
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-1.5">
            <Label htmlFor="queue-scope">Deduplication scope</Label>
            <Select value={f.scope} onValueChange={(scope) => set({ scope: scope as QueueForm["scope"], highThroughput: scope === "messageGroup" && f.limit === "perMessageGroupId" })}>
              <SelectTrigger id="queue-scope" className="w-full bg-card font-mono text-[13px]" data-testid="queue-scope"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="queue">queue</SelectItem>
                <SelectItem value="messageGroup">messageGroup</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="queue-limit">FIFO throughput limit</Label>
            <Select value={f.limit} onValueChange={(limit) => set({ limit: limit as QueueForm["limit"], highThroughput: limit === "perMessageGroupId" && f.scope === "messageGroup" })}>
              <SelectTrigger id="queue-limit" className="w-full bg-card font-mono text-[13px]" data-testid="queue-limit"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="perQueue">perQueue</SelectItem>
                <SelectItem value="perMessageGroupId">perMessageGroupId</SelectItem>
              </SelectContent>
            </Select>
            <LocalStatus state="stored" reason="FIFO throughput limit is validated and returned; nothing is throttled." />
          </div>
        </div>
      </div>
    </>
  );
}
