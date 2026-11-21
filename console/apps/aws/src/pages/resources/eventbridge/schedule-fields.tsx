// The schedule of a scheduled rule (eventbridge__rule-create-schedule): a cron expression in its six fields, or a rate; what it says in words and the next
// ten times it fires, in UTC or in the browser's own zone.
import { useMemo, useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { FormSection, Field, RadioCards, RuleHelp, Segmented } from "../../../kit";
import { CRON_PARTS, type CronFields, cronProblem, cronText, cronWords, localRunWords, nextRuns, parseCron, parseRate, runWords, values } from "./cron";

export type ScheduleMode = "cron" | "rate";
const LABELS: Record<keyof CronFields, string> = { minutes: "Minutes", hours: "Hours", dom: "Day of month", month: "Month", dow: "Day of week", year: "Year" };
const FIRST: CronFields = { minutes: "0", hours: "18", dom: "?", month: "*", dow: "MON-FRI", year: "*" };

/** What is wrong with the expression, in words; `null` when it can be sent. */
export function scheduleProblem(expression: string): string | null {
  if (expression.startsWith("rate(")) {
    const r = parseRate(expression);
    return "problem" in r ? r.problem : null;
  }
  const f = parseCron(expression);
  if (!f) return "A schedule is cron(…) with six fields, or rate(value unit).";
  const own = cronProblem(f);
  if (own) return own;
  const bad = CRON_PARTS.find((k) => values(f[k], k) === null && !/[LW#]/.test(f[k]));
  return bad ? `${LABELS[bad]} is not a valid value: ${f[bad]}.` : null;
}

export function ScheduleFields({ value, onChange, tried }: { value: string; onChange: (v: string) => void; tried: boolean }) {
  const [mode, setMode] = useState<ScheduleMode>(value.startsWith("cron(") ? "cron" : "rate");
  const [zone, setZone] = useState<"utc" | "local">("utc");
  const cron = parseCron(value) ?? FIRST;
  const rate = /^rate\((\d+) (\w+)\)$/.exec(value);
  const problem = scheduleProblem(value);
  const runs = useMemo(() => (mode === "cron" && parseCron(value) && !cronProblem(parseCron(value)!) ? nextRuns(parseCron(value)!, Date.now(), 10) : null), [mode, value]);
  const pick = (m: ScheduleMode) => {
    setMode(m);
    onChange(m === "cron" ? cronText(FIRST) : "rate(5 minutes)");
  };
  return (
    <FormSection title="Schedule pattern" description="Run at specific times, or at a regular rate." testId="section-schedule">
      <RadioCards<ScheduleMode> name="schedule-mode" label="Schedule pattern" value={mode} onChange={pick} options={[{ value: "cron", title: "A fine-grained schedule", body: "A fine-grained schedule that runs at a specific time, such as 8:00 a.m. PST on the first Monday of every month." }, { value: "rate", title: "A schedule that runs at a regular rate", body: "A schedule that runs at a regular rate, such as every 10 minutes." }]} />
      {mode === "cron" ? (
        <div className="grid gap-4" data-testid="cron-fields">
          <p className="text-[13.5px] font-medium text-bright">Cron expression</p>
          <div className="grid gap-3 sm:grid-cols-3 lg:grid-cols-6">
            {CRON_PARTS.map((k) => (
              <div key={k} className="grid gap-1.5">
                <Label htmlFor={`cron-${k}`}>{LABELS[k]}</Label>
                <Input id={`cron-${k}`} value={cron[k]} onChange={(e) => onChange(cronText({ ...cron, [k]: e.target.value.trim() }))} autoComplete="off" spellCheck={false} className="font-mono" data-testid={`cron-${k}`} />
              </div>
            ))}
          </div>
          <RuleHelp>Day of month and day of week can't both have a value: put <span className="font-mono">?</span> in one. The finest resolution is <b>one minute</b>, and every time is <b>UTC</b>.</RuleHelp>
          {problem ? (tried || cronProblem(cron)) && <p className="text-[13px] text-bad" role="alert" data-testid="schedule-problem">{problem}</p> : <p className="text-[13.5px]" data-testid="cron-words">This is <span className="font-mono font-semibold text-bright">{value}</span>: {cronWords(cron).replace(/^./, (c) => c)}.</p>}
          {!problem && (
            <div data-testid="next-runs">
              <div className="mb-2 flex flex-wrap items-center justify-between gap-3">
                <h3 className="text-[14px] font-semibold text-bright">Next ten dates</h3>
                <Segmented<"utc" | "local"> label="Time zone" value={zone} onChange={setZone} options={[{ value: "utc", label: "UTC" }, { value: "local", label: "Local time zone" }]} testId="zone" />
              </div>
              {runs === null ? <p className="text-[13px] text-muted-foreground">The dates of an expression with L, W or # are not listed here; the service takes it as written.</p> : runs.length === 0 ? <p className="text-[13px] text-muted-foreground">This expression never fires.</p> : (
                <ol className="grid gap-x-8 gap-y-1 font-mono text-[13px] sm:grid-cols-2" data-testid="run-list">
                  {runs.map((t) => <li key={t}>{zone === "utc" ? runWords(t) : localRunWords(t)}</li>)}
                </ol>
              )}
            </div>
          )}
        </div>
      ) : (
        <div className="grid gap-3" data-testid="rate-fields">
          <RuleHelp className="mb-1">doze-aws fires a schedule at the first slot after it first sees the rule, and only for a rule that has a target; what a restart or a long pause missed is not replayed, and a rate counts from the rule's creation (<span className="font-mono">crates/aws-eventbridge/src/schedule.rs</span>).</RuleHelp>
          <Field label="Rate expression" error={problem && (tried || !rate) ? problem : undefined} help={<>A rate expression is <span className="font-mono">rate(value unit)</span>: a positive number and <b>minute</b>, <b>hour</b> or <b>day</b>, singular for 1 and plural above it, so <span className="font-mono">rate(1 hour)</span> and <span className="font-mono">rate(5 hours)</span> are valid.</>}>
            {(id) => <Input id={id} value={value} onChange={(e) => onChange(e.target.value)} autoComplete="off" spellCheck={false} className="max-w-xs font-mono" data-testid="rate-expression" />}
          </Field>
        </div>
      )}
    </FormSection>
  );
}
