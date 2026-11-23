// Create alarm (cloudwatch__alarm-step1 to step4), four steps; also Edit alarm (the same pages over an alarm that exists, its name
// kept). Nothing is sent until the last step: CreateTopic for a topic made on the way, then PutMetricAlarm.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { DetailError, FormPage, callKindOp, useServiceCrumbs } from "../../../kit";
import { useAlarm, useAlarms, usePutAlarm } from "./api";
import { ActionsStep } from "./wizard-actions";
import { type Draft, type Notify, actionsInput, arnOf, empty, fromAlarm, fromSel, metricInput, nameInput, nextId, problem, wholeInput } from "./wizard-model";
import { MetricStep } from "./wizard-metric";
import { NameStep, ReviewStep } from "./wizard-review";

const STEPS = [{ id: "metric", label: "Specify metric and conditions" }, { id: "actions", label: "Configure actions" }, { id: "name", label: "Add name and description" }, { id: "review", label: "Preview and create" }];
const SUBS = [
  "Step 1 of 4. Choose the metric to watch and the condition that puts the alarm in ALARM.",
  "Step 2 of 4. Choose what happens when the alarm changes state.",
  "Step 3 of 4. Name the alarm and say what it means.",
  "Step 4 of 4. Check everything, then create the alarm. Creating it sets its state to INSUFFICIENT_DATA until the first evaluation.",
];

/** The words under the title: the board's, and for an edit the alarm's name and what the last step does. */
function subOf(step: number, editing: string | undefined): string {
  if (editing === undefined) return SUBS[step]!;
  return step === 3 ? "Step 4 of 4. Check everything, then update the alarm. Its state and history are kept." : SUBS[step]!.replace(/^Step (\d) of 4\./, `Step $1 of 4. Editing ${editing}.`);
}

export function CreateAlarm({ editing }: { editing?: string }) {
  const search = useSearch({ strict: false }) as { ns?: string | number; metric?: string | number; dims?: string | string[]; stat?: string; period?: number };
  const existing = useAlarm(editing ?? "", editing !== undefined);
  if (editing !== undefined) {
    if (existing.alarm === undefined) return <Skeleton className="h-96 rounded-[var(--radius-card)]" data-testid="wizard-loading" />;
    if (existing.alarm === null) return <DetailError service="cloudwatch" kind="alarms" name={editing} error={new ApiFailure(404, { code: "NOT_FOUND", message: `There is no alarm named ${editing}.` })} />;
    const draft = fromAlarm(existing.alarm);
    if (!draft) return <Alert tone="warn" title={`${editing} cannot be edited here`} testId="edit-unsupported">The wizard edits an alarm on one metric, or on one expression over it. Change this one with PutMetricAlarm or PutCompositeAlarm.</Alert>;
    return <Wizard initial={draft} editing={editing} />;
  }
  return <Wizard initial={fromSel(search) ?? empty()} />;
}

function Wizard({ initial, editing }: { initial: Draft; editing?: string }) {
  const navigate = useNavigate();
  const about = useAbout().data;
  const crumbs = useServiceCrumbs("cloudwatch");
  const put = usePutAlarm();
  const taken = useAlarms().data?.map((a) => a.AlarmName) ?? [];
  const [d, setD] = useState<Draft>(initial);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [failure, setFailure] = useState<{ code: string; message: string } | null>(null);
  const [working, setWorking] = useState(false);
  const [first, setFirst] = useState(true);
  const account = about?.account ?? "000000000000";
  const region = about?.region ?? "us-east-1";
  const arn = (n: Notify) => arnOf(n, account, region);
  const set = (patch: Partial<Draft>) => { setD((x) => ({ ...x, ...patch })); setFailure(null); };
  const stop = problem(step, d, taken, editing !== undefined);
  const go = (i: number) => {
    // The first look at the actions step opens with a notification to fill in, as the board does.
    if (i === 1 && first && d.notify.length === 0) set({ notify: [{ id: nextId(), state: "ALARM", via: "existing", arn: "", name: "" }] });
    if (i === 1) setFirst(false);
    setStep(i);
    setFurthest((f) => Math.max(f, i));
    setTried(false);
  };
  const next = () => { setTried(true); if (!stop) go(step + 1); };
  const back = () => (editing ? navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "alarms", name: editing } }) : navigate({ to: "/resources/$service/$second", params: { service: "cloudwatch", second: "alarms" }, search: {} }));
  const input = useMemo(() => {
    const [m, a, n] = [metricInput(d), actionsInput(d, arn), nameInput(d)];
    return [m, a, n, wholeInput(d, arn)][step]!;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [d, step, account, region]);
  const create = async () => {
    for (let i = 0; i < 3; i++) if (problem(i, d, taken, editing !== undefined)) { setStep(i); setTried(true); return; }
    setWorking(true);
    try {
      for (const n of d.notify.filter((x) => x.via === "new")) await callKindOp({ service: "sns", kind: "topics" }, "CreateTopic", { Name: n.name.trim() }).catch((e: unknown) => { if (!(e instanceof ApiFailure) || !/already exists|InvalidParameter/i.test(e.message)) throw e; });
      await put.mutateAsync(wholeInput(d, arn));
      const name = d.name.trim();
      void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "alarms", name }, search: { done: editing ? "updated" : "created" } });
    } catch (e) {
      setFailure({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : String(e) });
    } finally {
      setWorking(false);
    }
  };
  const title = editing ? "Edit alarm" : "Create alarm";
  return (
    <FormPage
      title={title}
      sub={subOf(step, editing)}
      stepsAfterTitle
      crumbs={[...crumbs, { label: "All alarms", to: "/resources/cloudwatch/alarms" }, { label: STEPS[step]!.label, to: "" }]}
      request={{ service: "cloudwatch", operation: "PutMetricAlarm", input }}
      showInput
      callNote={step < 3 ? <b className="font-medium text-foreground">PutMetricAlarm, so far</b> : undefined}
      refusal={failure}
      banner={failure ? <Alert tone="bad" title={`The alarm wasn't ${editing ? "updated" : "created"}.`} testId="create-refused">The service answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”.</Alert> : undefined}
      submitLabel={editing ? "Update alarm" : "Create alarm"}
      pending={working}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next }}
      backLabel="Previous"
      onCancel={() => void back()}
      onSubmit={() => void create()}
      testId="alarm-wizard"
    >
      {step === 0 && <MetricStep d={d} set={set} tried={tried} problem={stop} />}
      {step === 1 && <ActionsStep d={d} set={set} tried={tried} problem={stop} arn={arn} region={region} account={account} />}
      {step === 2 && <NameStep d={d} set={set} tried={tried} problem={stop} editing={editing !== undefined} />}
      {step === 3 && <ReviewStep d={d} go={go} arn={arn} />}
    </FormPage>
  );
}
