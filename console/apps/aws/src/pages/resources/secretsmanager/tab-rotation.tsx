// A secret's Rotation tab (secretsmanager__secret-rotation, rotation-edit): the rotation configuration, Rotate secret immediately, what
// a rotation does, and Edit rotation (a dialog: automatic rotation on or off, the schedule, the window, the function).
import { useQueryClient } from "@tanstack/react-query";
import { PencilIcon, RefreshCwIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { useAbout } from "../../../api/queries";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { callKindOp, Field, FunctionPicker, SectionEdit, Segmented } from "../../../kit";
import { Card, Facts, Note } from "../cfg-parts";
import { refusalOf } from "../kms/schedule";
import { SECRETS } from "./api";
import { DurationSelect } from "./duration-select";
import { type Cadence, type Described, cadenceOf, cadenceProblem, dayOf, durationProblem, expressionOf, functionName, scheduleProblem, scheduleWords, shortId } from "./model";

export function TabRotation({ name, d, scheduled }: { name: string; d: Described; scheduled: boolean }) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [done, setDone] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const reread = () => Promise.all([qc.invalidateQueries({ queryKey: ["resources", "kindop", "secretsmanager"] }), qc.invalidateQueries({ queryKey: ["resources", "one", "secretsmanager", "secrets"] })]);
  const rotate = async () => {
    setPending(true);
    setRefused(null);
    setDone(false);
    try {
      await callKindOp(SECRETS, "RotateSecret", { SecretId: name });
      setDone(true);
      await reread();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const fn = d.RotationLambdaARN ? functionName(d.RotationLambdaARN) : null;
  const current = Object.entries(d.VersionIdsToStages ?? {}).find(([, s]) => s.includes("AWSCURRENT"))?.[0];
  return (
    <div className="grid gap-5" data-testid="rotation-tab">
      {done && <ResultBanner title="Rotation started." returned="RotateSecret answered at once; the rotation runs after it, and the Versions tab shows the new version when it finishes." onDismiss={() => setDone(false)} testId="rotation-started" />}
      {refused && <Alert tone="bad" title={`RotateSecret was refused: ${refused.code}`} testId="rotate-refused">{refused.message}</Alert>}
      <Card title="Rotation configuration" lede={fn ? "Secrets Manager calls the rotation function each time a rotation runs." : "Rotation is off. Edit rotation to choose a function and a schedule."} testId="rotation-config" actions={<>
        <Button type="button" variant="outline" className="bg-card" onClick={() => void rotate()} disabled={pending || !d.RotationEnabled || scheduled} data-testid="rotate-now"><RefreshCwIcon /> {pending ? "Rotating…" : "Rotate secret immediately"}</Button>
        <Button type="button" variant="outline" className="bg-card" onClick={() => setOpen(true)} disabled={scheduled} data-testid="rotation-edit-open"><PencilIcon /> Edit rotation</Button>
      </>}>
        <Facts facts={[
          { label: "Rotation status", value: d.RotationEnabled ? "Enabled" : "Off", testId: "fact-status" },
          { label: "Rotation schedule", value: d.RotationEnabled ? (<><span className="font-mono text-[13px]">{scheduleWords(d) ?? "–"}</span>{d.RotationRules?.Duration && <span className="ml-2 text-muted-foreground">Window duration {d.RotationRules.Duration}. Stored and shown: the rotation starts when the window opens, whatever its length.</span>}</>) : "–" },
          { label: "Lambda rotation function", value: <span className="font-mono text-[13px]">{d.RotationLambdaARN ?? "–"}</span> },
          { label: "Last rotated date", value: dayOf(d.LastRotatedDate) ?? "–" },
          { label: "Next rotation date", value: dayOf(d.NextRotationDate) ?? "–" },
        ]} />
      </Card>
      <Note testId="rotation-note"><b>What a rotation does.</b> Secrets Manager calls {fn ? <span className="font-mono">{fn}</span> : "the rotation function"} four times: createSecret writes the new value as a version labeled AWSPENDING, setSecret changes it in the service, testSecret checks it, and finishSecret moves AWSCURRENT to it.{current && d.LastRotatedDate ? ` The last rotation made ${shortId(current)} current.` : ""} A rotation runs when its schedule opens a window, and when you choose Rotate secret immediately or make a RotateSecret call; RotateSecret answers at once and the steps run after it. The window&apos;s duration is stored and shown, and changes nothing locally.</Note>
      {open && <RotationEdit name={name} d={d} onClose={() => setOpen(false)} />}
    </div>
  );
}

function RotationEdit({ name, d, onClose }: { name: string; d: Described; onClose: () => void }) {
  const qc = useQueryClient();
  const [on, setOn] = useState(Boolean(d.RotationEnabled));
  const [schedule, setSchedule] = useState(d.RotationRules?.ScheduleExpression ?? (d.RotationRules?.AutomaticallyAfterDays ? `rate(${d.RotationRules.AutomaticallyAfterDays} days)` : "rate(30 days)"));
  const [duration, setDuration] = useState(d.RotationRules?.Duration ?? "");
  const about = useAbout().data;
  const [fn, setFn] = useState<string | null>(d.RotationLambdaARN ? functionName(d.RotationLambdaARN) : null);
  const arn = fn ? `arn:aws:lambda:${about?.region ?? "us-east-1"}:${about?.account ?? ""}:function:${fn}` : "";
  const [now, setNow] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  // The builder says a rate in days or hours; an expression it cannot say (a cron) opens on the expression.
  const [how, setHow] = useState<"builder" | "expression">(cadenceOf(schedule) || schedule.trim() === "" ? "builder" : "expression");
  const [cadence, setCadence] = useState<Cadence>(cadenceOf(schedule) ?? { n: "30", unit: "days" });
  const built = (c: Cadence) => { setCadence(c); setSchedule(expressionOf(c)); };
  const switchTo = (v: "builder" | "expression") => { if (v === "builder") { const c = cadenceOf(schedule); if (c) setCadence(c); else built(cadence); } setHow(v); };
  const scheduleBad = how === "builder" ? cadenceProblem(cadence) : scheduleProblem(schedule);
  const problem = on ? (fn ? (scheduleBad ?? durationProblem(duration)) : "Choose the Lambda function that rotates this secret.") : null;
  const input = on ? { SecretId: name, RotationLambdaARN: arn, RotationRules: { ScheduleExpression: schedule, ...(duration ? { Duration: duration } : {}) }, RotateImmediately: now } : { SecretId: name };
  const save = async () => {
    setPending(true);
    setError(null);
    try {
      await callKindOp(SECRETS, on ? "RotateSecret" : "CancelRotateSecret", input);
      await Promise.all([qc.invalidateQueries({ queryKey: ["resources", "kindop", "secretsmanager"] }), qc.invalidateQueries({ queryKey: ["resources", "one", "secretsmanager", "secrets"] })]);
      onClose();
    } catch (e) {
      setError(e);
    } finally {
      setPending(false);
    }
  };
  return (
    <SectionEdit open wide onOpenChange={(o) => { if (!o) onClose(); }} title="Edit rotation configuration" request={{ service: "secretsmanager", operation: on ? "RotateSecret" : "CancelRotateSecret", input }} onSave={() => void save()} pending={pending} error={error} disabled={problem !== null} disabledWhy={problem} testId="edit-rotation">
      <div className="flex items-center gap-3"><Switch id="rotation-on" checked={on} onCheckedChange={setOn} data-testid="rotation-on" /><Label htmlFor="rotation-on" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Automatic rotation</span><span className="text-[13px] text-muted-foreground">Switch it off to turn rotation off.</span></Label></div>
      {on && (
        <>
          <div className="grid gap-2" data-testid="rotation-schedule-group">
            <span className="text-[14px] leading-none font-medium text-bright">Rotation schedule</span>
            <Segmented value={how} onChange={switchTo} label="How to write the schedule" options={[{ value: "builder", label: "Schedule expression builder" }, { value: "expression", label: "Schedule expression" }]} className="w-fit" testId="schedule-how" />
            {how === "builder" ? (
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-[13.5px] text-muted-foreground">Rotate every</span>
                <Input inputMode="numeric" value={cadence.n} onChange={(e) => built({ ...cadence, n: e.target.value })} aria-label="Number" aria-invalid={cadenceProblem(cadence) !== null} className="w-24 bg-card font-mono tabular-nums" data-testid="cadence-n" />
                <Select value={cadence.unit} onValueChange={(u) => built({ ...cadence, unit: u as Cadence["unit"] })}>
                  <SelectTrigger className="w-28 bg-card" aria-label="Unit" data-testid="cadence-unit"><SelectValue /></SelectTrigger>
                  <SelectContent><SelectItem value="days">days</SelectItem><SelectItem value="hours">hours</SelectItem></SelectContent>
                </Select>
                <span className="font-mono text-[13px] text-muted-foreground" data-testid="cadence-expression">{expressionOf(cadence)}</span>
              </div>
            ) : (
              <Input value={schedule} onChange={(e) => setSchedule(e.target.value)} aria-label="Schedule expression" className="bg-card font-mono" data-testid="rotation-schedule" />
            )}
            {problem && fn && scheduleBad && <p className="text-[13px] text-bad" role="alert">{scheduleBad}</p>}
            <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">UTC, written as rate() or cron(), for example <span className="font-mono">rate(30 days)</span>. doze-aws starts a rotation when the schedule opens its window, and runs it when you choose Rotate secret immediately.</p>
          </div>
          <Field label="Window duration" optional help="The length of the rotation window in hours, for example 3h. It is stored and shown, and it changes nothing here: doze-aws starts the rotation when the window opens.">
            {(id) => <DurationSelect id={id} value={duration} onChange={setDuration} />}
          </Field>
          <div className="flex items-start gap-3"><Checkbox id="rotate-now" checked={now} onCheckedChange={(v) => setNow(v === true)} className="mt-0.5" data-testid="rotate-immediately" /><Label htmlFor="rotate-now" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Rotate immediately when the secret is stored</span><span className="text-[13px] text-muted-foreground">Choose it to rotate the secret when you save your changes.</span></Label></div>
          <Field label="Rotation function" help="doze-aws invokes this Lambda function for each of the four rotation steps.">
            {(id) => <FunctionPicker id={id} value={fn} onChange={setFn} by="name" />}
          </Field>
        </>
      )}
    </SectionEdit>
  );
}

