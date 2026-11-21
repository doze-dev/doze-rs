// Start DLQ redrive (sqs__dlq-redrive): where the messages go and how fast, with the call it will make; the task it starts is shown
// on the queue's Dead-letter queue tab (./redrive-status.tsx).
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import { Link } from "@tanstack/react-router";
import { MAX_RATE, startInput, startProblem } from "./redrive-form";
import type { SubProps } from "./sub-pages";

export function SqsRedrive({ d, q, back, done }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const about = useAbout().data;
  const [where, setWhere] = useState<"source" | "custom">("source");
  const [dest, setDest] = useState<string | null>(null);
  const [rate, setRate] = useState<"system" | "custom">("system");
  const [per, setPer] = useState("");
  const start = useOp<{ TaskHandle?: string }>(t, "StartMessageMoveTask");
  const list = useResourceList("sqs", "queues");
  const sources = d.subs.find((s) => s.title.includes("dead-letter queue"))?.rows.map((r) => r.name) ?? [];
  const peers = (list.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => r.name !== d.name && (r.cells[0] === "FIFO") === q.fifo);
  const arn = (n: string) => `arn:aws:sqs:${about?.region ?? "us-east-1"}:${about?.account ?? "000000000000"}:${n}`;
  const form = { where, dest, rate, per };
  const problem = startProblem(form);
  const input = startInput(q.arn, form);
  const e = start.error;
  const refused = e instanceof ApiFailure ? { code: e.code, message: e.message } : e ? { code: "Error", message: (e as Error).message } : null;
  return (
    <FormPage
      title="Start DLQ redrive"
      sub={`Move the messages out of ${q.name}. SQS starts a task that receives them from this queue and sends them on.`}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }]}
      mono
      request={{ service: "sqs", operation: "StartMessageMoveTask", input }}
      refusal={refused}
      callNote={<>{where === "source" ? "No DestinationArn, so each message returns to its original source queue." : "A DestinationArn, so every message goes there."} {rate === "system" ? "No rate, so it moves at the system's pace (doze-aws moves 500 a second)." : `At most ${per || "?"} messages a second.`}</>}
      banner={refused ? <Alert tone="bad" title="The redrive was not started." testId="redrive-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Redrive messages"
      pending={start.isPending}
      disabled={problem !== null}
      disabledWhy={problem}
      onCancel={back}
      onSubmit={() => start.mutate(input, { onSuccess: () => done("redrive-started") })}
      testId="redrive-form"
    >
      <FormSection title="Redrive configuration" description="From ListDeadLetterSourceQueues.">
        <RadioGroup value={where} onValueChange={(v) => setWhere(v as "source" | "custom")} className="grid gap-2" aria-label="Message destination">
          {[["source", "Redrive to source queue(s)", "Each message goes back to the queue it came from."], ["custom", "Redrive to custom destination", "Enter the ARN of another queue of the same type."]].map(([v, title, text]) => (
            <Label key={v} htmlFor={`where-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
              <RadioGroupItem id={`where-${v}`} value={v!} className="mt-0.5" data-testid={`where-${v}`} />
              <span className="grid gap-0.5"><span className="font-medium text-bright">{title}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
            </Label>
          ))}
        </RadioGroup>
        {where === "custom" && (
          <Field label="Destination queue" hint="Choose a queue, or enter the ARN of one that isn't listed. It must be the same type as this queue." testId="field-destination">
            {(id) => <Combobox id={id} options={peers.map((r) => ({ value: arn(r.name), label: r.label }))} value={dest} onChange={setDest} placeholder="Choose a queue" search="Search queues, or enter an ARN…" empty="No queue matches." className="w-full font-mono" pattern aria-label="Destination queue" />}
          </Field>
        )}
        <p className="text-[13.5px] text-muted-foreground" data-testid="redrive-sources">
          Source queues{" "}
          {sources.length > 0 ? sources.map((s, i) => <span key={s}>{i > 0 && ", "}<Link to="/resources/$service/$kind/$name" params={{ service: "sqs", kind: "queues", name: s }} className="font-mono text-link hover:underline">{s}</Link></span>) : "none"}
          . A message sent straight to this queue has no source queue to return to, and the task fails. Choose a custom destination for those; it must be the same type as this queue.
        </p>
      </FormSection>
      <FormSection title="Velocity control settings">
        <RadioGroup value={rate} onValueChange={(v) => setRate(v as "system" | "custom")} className="grid gap-2" aria-label="Velocity">
          {[["system", "System optimized", "Redrive at the maximum number of messages per second. AWS tunes the rate to the backlog; doze-aws moves 500 a second."], ["custom", "Custom max velocity", `Redrive at a rate you set. The most allowed is ${MAX_RATE} messages per second.`]].map(([v, title, text]) => (
            <Label key={v} htmlFor={`rate-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
              <RadioGroupItem id={`rate-${v}`} value={v!} className="mt-0.5" data-testid={`rate-${v}`} />
              <span className="grid gap-0.5"><span className="font-medium text-bright">{title}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
            </Label>
          ))}
        </RadioGroup>
        {rate === "custom" && (
          <Field label="Custom max velocity" help="Start with a small value, check that the queue receiving the messages is not overwhelmed, then raise it gradually. Used only with Custom max velocity." testId="field-velocity">
            {(id) => (
              <div className="flex items-center gap-2"><Input id={id} value={per} onChange={(ev) => setPer(ev.target.value)} inputMode="numeric" className="w-32 font-mono" autoComplete="off" data-testid="redrive-rate" /><span className="text-[13.5px] text-muted-foreground">messages per second</span></div>
            )}
          </Field>
        )}
        <RuleHelp><b>Before you start.</b> You can't filter or change messages while redriving. Only one task can run on a queue at a time, and what a task has moved stays moved if you cancel it.</RuleHelp>
        <RuleHelp>On AWS the caller needs sqs:StartMessageMoveTask, sqs:ReceiveMessage, sqs:DeleteMessage and sqs:GetQueueAttributes on the dead-letter queue, and sqs:SendMessage on the destination queue. doze-aws consults IAM according to its mode: soft (the default) records a missing permission and enforce refuses.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}
