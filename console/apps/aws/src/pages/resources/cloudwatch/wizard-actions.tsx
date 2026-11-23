// Step 2 of the alarm wizard (cloudwatch__alarm-step2): what happens when the alarm changes state. A notification goes to an SNS
// topic (one that exists, one to make, or an ARN); a Lambda function can be run; the other actions of AWS are drawn off, with why.
import { PlusIcon, Trash2Icon } from "lucide-react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Label } from "@doze/ui/components/ui/label";
import { Field, FormSection, FunctionPicker, RadioCards, TopicPicker } from "../../../kit";
import { useResourceDetail } from "../../../api/resources";
import { STATES } from "./model";
import { type Draft, type Notify, type StateName, nextId } from "./wizard-model";
import { subscribers } from "./wizard-words";

type Props = { d: Draft; set: (patch: Partial<Draft>) => void; tried: boolean; arn: (n: Notify) => string; problem: string | null; region: string; account: string };

const STATE_BODY: Record<StateName, string> = {
  ALARM: "The metric or expression is outside of the defined threshold.",
  OK: "The metric or expression is within the defined threshold.",
  INSUFFICIENT_DATA: "The alarm has just started, the metric is not available, or not enough data is available for the metric to determine the alarm state.",
};

const VIA = [["existing", "Select an existing SNS topic"], ["new", "Create a new SNS topic"], ["arn", "Use a topic ARN"]] as const;

function TopicWords({ name }: { name: string }) {
  const t = useResourceDetail("sns", "topics", name, name !== "");
  const subs = t.data?.subs.find((s) => s.title === "Subscriptions")?.rows;
  return (
    <p className="text-[13px] leading-5 text-muted-foreground" data-testid="topic-note">
      <b className="font-semibold text-foreground">CloudWatch does not test the topic.</b> It neither checks that the action exists nor detects SNS errors when it is invoked.{" "}
      {name !== "" && subs && <Emphasis text={subscribers(name, subs.map((r) => ({ protocol: r.cells[1] ?? "", endpoint: r.cells[2] ?? "" })))} names={[name]} />}
    </p>
  );
}

function Notification({ n, d, set, region, account, tried, problem }: Props & { n: Notify }) {
  const patch = (p: Partial<Notify>) => set({ notify: d.notify.map((x) => (x.id === n.id ? { ...x, ...p } : x)) });
  const remove = () => set({ notify: d.notify.filter((x) => x.id !== n.id) });
  const topicName = n.via === "new" ? "" : n.arn.split(":").pop() ?? "";
  const bad = tried && problem && ((n.via === "existing" && n.arn === "") || (n.via === "new" && !/^[A-Za-z0-9_-]+$/.test(n.name)) || (n.via === "arn" && !n.arn)) ? problem : undefined;
  return (
    <div className="grid gap-5 border-t pt-5 first:border-t-0 first:pt-0" data-testid={`notification-${n.id}`}>
      <div className="grid gap-2">
        <span className="text-[14px] font-medium text-bright">Alarm state</span>
        <RadioGroup value={n.state} onValueChange={(v) => patch({ state: v as StateName })} className="grid gap-3" aria-label="Alarm state">
          {STATES.map((s) => (
            <Label key={s} htmlFor={`state-${n.id}-${s}`} className="flex cursor-pointer items-start gap-3 font-normal leading-5">
              <RadioGroupItem id={`state-${n.id}-${s}`} value={s} className="mt-0.5" data-testid={`state-${s.toLowerCase()}`} />
              <span className="grid gap-0.5"><span className="font-medium text-bright">{s}</span><span className="text-[13px] text-muted-foreground">{STATE_BODY[s]}</span></span>
            </Label>
          ))}
        </RadioGroup>
      </div>
      <div className="grid gap-2">
        <span className="text-[14px] font-medium text-bright">SNS topic to notify</span>
        <RadioGroup value={n.via} onValueChange={(v) => patch({ via: v as Notify["via"], arn: "", name: "" })} className="grid gap-3" aria-label="SNS topic to notify">
          {VIA.map(([v, label]) => (
            <Label key={v} htmlFor={`via-${n.id}-${v}`} className="flex cursor-pointer items-center gap-2 font-normal"><RadioGroupItem id={`via-${n.id}-${v}`} value={v} data-testid={`via-${v}`} />{label}</Label>
          ))}
        </RadioGroup>
      </div>
      {n.via === "existing" && (
        <Field label="Topic" error={bad}>
          {(id) => (
            <div className="grid gap-1.5">
              <TopicPicker id={id} by="id" value={n.arn || null} onChange={(v) => patch({ arn: v ?? "" })} aria-label="Topic" />
              {n.arn && <span className="font-mono text-[12.5px] break-all text-muted-foreground" data-testid="topic-arn">{n.arn}</span>}
            </div>
          )}
        </Field>
      )}
      {n.via === "new" && (
        <Field label="Topic name" error={bad} hint={<>The topic is created when you create the alarm: <span className="font-mono">{`arn:aws:sns:${region}:${account}:${n.name || "<name>"}`}</span>. Add its subscriptions from the topic's page.</>}>
          {(id) => <Input id={id} value={n.name} onChange={(e) => patch({ name: e.target.value })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="new-topic-name" />}
        </Field>
      )}
      {n.via === "arn" && (
        <Field label="Topic ARN" error={bad}>
          {(id) => <Input id={id} value={n.arn} onChange={(e) => patch({ arn: e.target.value })} placeholder={`arn:aws:sns:${region}:${account}:alerts`} className="font-mono" autoComplete="off" spellCheck={false} data-testid="topic-arn-input" />}
        </Field>
      )}
      <TopicWords name={n.via === "existing" ? topicName : ""} />
      <div><Button type="button" variant="outline" className="bg-card" onClick={remove} data-testid="notification-remove"><Trash2Icon /> Remove</Button></div>
    </div>
  );
}

function LambdaAction({ n, d, set, arn, account, region, tried, problem }: Props & { n: Notify }) {
  const patch = (p: Partial<Notify>) => set({ notify: d.notify.map((x) => (x.id === n.id ? { ...x, ...p } : x)) });
  return (
    <div className="grid gap-4 border-t pt-5 first:border-t-0 first:pt-0" data-testid={`lambda-${n.id}`}>
      <RadioCards<StateName> name={`lstate-${n.id}`} label="Alarm state" value={n.state} onChange={(state) => patch({ state })} options={STATES.map((s) => ({ value: s, title: s }))} className="sm:grid-cols-3" />
      <Field label="Lambda function" error={tried && n.arn === "" ? problem ?? undefined : undefined}>
        {(id) => (
          <div className="grid gap-1.5">
            <FunctionPicker id={id} by="name" value={n.arn ? n.arn.split(":function:")[1]?.split(":")[0] ?? null : null} onChange={(v) => patch({ arn: v ? `arn:aws:lambda:${region}:${account}:function:${v}` : "" })} aria-label="Lambda function" />
            {arn(n) && <span className="font-mono text-[12.5px] break-all text-muted-foreground">{arn(n)}</span>}
          </div>
        )}
      </Field>
      <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ notify: d.notify.filter((x) => x.id !== n.id) })} data-testid="lambda-remove"><Trash2Icon /> Remove</Button></div>
    </div>
  );
}

const OFF = ["Auto Scaling action", "EC2 action", "Add Systems Manager action", "Add investigation action"];

export function ActionsStep(p: Props) {
  const { d, set } = p;
  return (
    <>
      <FormSection title="Notification" description="Send a message to an SNS topic when the alarm enters a state." testId="section-notification">
        {d.notify.filter((n) => n.via !== "lambda").map((n) => <Notification key={n.id} n={n} {...p} />)}
        <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ notify: [...d.notify, { id: nextId(), state: "ALARM", via: "existing", arn: "", name: "" }] })} data-testid="notification-add"><PlusIcon /> Add notification</Button></div>
      </FormSection>
      <FormSection title="Actions" description="Each of these adds an action for the state you pick. An alarm can have at most five actions per state." testId="section-actions">
        {d.notify.filter((n) => n.via === "lambda").map((n) => <LambdaAction key={n.id} n={n} {...p} />)}
        <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ notify: [...d.notify, { id: nextId(), state: "ALARM", via: "lambda", arn: "", name: "" }] })} data-testid="lambda-add"><PlusIcon /> Add Lambda action</Button></div>
        <p className="text-[13px] text-muted-foreground">Name or ARN of the function, and optionally a version.</p>
        <div className="flex flex-wrap gap-2 border-t pt-5">
          {OFF.map((label) => <Button key={label} type="button" variant="outline" className="bg-card" disabled title="Not available in doze-aws">{label}</Button>)}
        </div>
        <p className="text-[13px] leading-5 text-muted-foreground" data-testid="actions-off-reason"><b className="font-semibold text-foreground">Not available in doze-aws:</b> alarm actions are delivered to SNS topics and Lambda functions only; an action it cannot deliver would look wired up and do nothing.</p>
      </FormSection>
    </>
  );
}
