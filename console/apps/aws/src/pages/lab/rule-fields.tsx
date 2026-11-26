import { useMemo } from "react";
import { MultiSelect } from "@doze/ui/components/app/multi-select";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { cn } from "@doze/ui/lib/utils";
import { useResourcesIndex } from "../../api/resources";
import type { LabProblem } from "../../api/types/LabProblem";
import type { LabTarget } from "../../api/types/LabTarget";
import type { RegionRow } from "../../api/types/RegionRow";
import { Segmented } from "../../kit";
import { type RuleForm, type Unit } from "./rule-form";

type Set = (patch: Partial<RuleForm>) => void;

/** A field with its label, a line of help, and what is wrong with it in the words the server gave (by the field's name). */
export function Field({ id, label, help, problems, field, children, className }: { id: string; label: string; help?: React.ReactNode; problems: LabProblem[]; field: string; children: React.ReactNode; className?: string }) {
  const mine = problems.filter((p) => p.field === field);
  return (
    <div className={cn("grid gap-1.5", className)} data-field={field}>
      <Label htmlFor={id}>{label}</Label>
      {children}
      {help && mine.length === 0 && <p className="text-[12.5px] text-muted-foreground">{help}</p>}
      {mine.map((p) => <p key={p.text} className="text-[12.5px] text-destructive" role="alert" data-testid={`problem-${field}`}>{p.text}</p>)}
    </div>
  );
}

const NumberField = ({ id, value, onChange, label, suffix, invalid }: { id: string; value: string; onChange: (v: string) => void; label: string; suffix?: string; invalid?: boolean }) => (
  <div className="flex items-center gap-2">
    <Input id={id} inputMode="numeric" value={value} onChange={(e) => onChange(e.target.value)} aria-label={label} aria-invalid={invalid} className="h-9 w-28 bg-card font-mono" data-testid={id} />
    {suffix && <span className="text-[13px] text-muted-foreground">{suffix}</span>}
  </div>
);

/** Which calls: the services, operations, resources, regions and callers a rule fits. Each is a list; an empty one fits anything. */
export function MatchFields({ form, set, problems, targets, regions }: { form: RuleForm; set: Set; problems: LabProblem[]; targets: LabTarget[]; regions: RegionRow[] }) {
  const index = useResourcesIndex(true, false);
  const services = useMemo(() => {
    const seen = new Map<string, string>();
    for (const t of targets) if (!seen.has(t.service)) seen.set(t.service, t.title);
    return [...seen].map(([value, label]) => ({ value, label, hint: value }));
  }, [targets]);
  const operations = useMemo(() => [...new Set(targets.filter((t) => form.services.includes(t.service)).flatMap((t) => t.operations))].sort().map((o) => ({ value: o, label: o })), [targets, form.services]);
  const names = useMemo(() => {
    const all = (index.data?.services ?? []).filter((s) => form.services.includes(s.name)).flatMap((s) => s.kinds.flatMap((k) => k.names));
    return [...new Set(all)].sort().map((n) => ({ value: n, label: n }));
  }, [index.data, form.services]);
  const regionOptions = regions.map((r) => ({ value: r.code, label: r.code, hint: r.name }));
  return (
    <div className="grid gap-4 sm:grid-cols-2" data-testid="rule-match">
      <Field id="rule-services" label="Services" field="services" problems={problems} help="Empty fits every service.">
        <MultiSelect options={services} value={form.services} onChange={(services) => set({ services })} noun="services" aria-label="Services" triggerClassName="w-full" pattern />
      </Field>
      <Field id="rule-operations" label="Operations" field="operations" problems={problems} help={form.services.length === 0 ? "Name a service to choose from its operations, or type an exact name." : "Empty fits every operation of those services."}>
        <MultiSelect options={operations} value={form.operations} onChange={(operations) => set({ operations })} noun="operations" aria-label="Operations" triggerClassName="w-full" pattern />
      </Field>
      <Field id="rule-resources" label="Resources" field="resources" problems={problems} help="A queue, table, bucket or function by name, or a pattern with * and ?. Empty fits every call, with or without one.">
        <MultiSelect options={names} value={form.resources} onChange={(resources) => set({ resources })} noun="resources" aria-label="Resources" triggerClassName="w-full" pattern />
      </Field>
      <Field id="rule-regions" label="Regions" field="regions" problems={problems} help="The region the call was signed for.">
        <MultiSelect options={regionOptions} value={form.regions} onChange={(regions) => set({ regions })} noun="regions" aria-label="Regions" triggerClassName="w-full" pattern />
      </Field>
      <Field id="rule-callers" label="Callers" field="callers" problems={problems} className="sm:col-span-2" help="Access key ids the call was signed with (the apps you point at doze-aws). Empty fits every caller.">
        <MultiSelect options={[]} value={form.callers} onChange={(callers) => set({ callers })} noun="callers" aria-label="Callers" triggerClassName="w-full sm:w-1/2" pattern />
      </Field>
    </div>
  );
}

/** What a Slow rule adds: how long, and how much it varies. */
export function SlowFields({ form, set, problems, max }: { form: RuleForm; set: Set; problems: LabProblem[]; max: number }) {
  const bad = problems.some((p) => p.field === "effect");
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <Field id="rule-delay" label="Delay" field="effect" problems={problems} help={`Up to ${max.toLocaleString("en")} ms.`}>
        <NumberField id="rule-delay" value={form.delay_ms} onChange={(delay_ms) => set({ delay_ms })} label="Delay in milliseconds" suffix="ms" invalid={bad} />
      </Field>
      <Field id="rule-jitter" label="Jitter" field="jitter" problems={problems} help="Each call waits the delay plus a random 0 up to this.">
        <NumberField id="rule-jitter" value={form.jitter_ms} onChange={(jitter_ms) => set({ jitter_ms })} label="Jitter in milliseconds" suffix="ms" />
      </Field>
    </div>
  );
}

/** How many of the calls that fit: every one, one in n, or a share. */
export function ShareFields({ form, set, problems }: { form: RuleForm; set: Set; problems: LabProblem[] }) {
  return (
    <Field id="rule-share" label="How often" field="share" problems={problems}>
      <div className="flex flex-wrap items-center gap-3">
        <Segmented label="How often" value={form.share} onChange={(share) => set({ share })} options={[{ value: "every", label: "Every call" }, { value: "nth", label: "One in n" }, { value: "percent", label: "A share" }]} testId="rule-share" />
        {form.share === "nth" && <NumberField id="rule-n" value={form.n} onChange={(n) => set({ n })} label="Every nth call" suffix="calls: every nth one" />}
        {form.share === "percent" && <NumberField id="rule-percent" value={form.percent} onChange={(percent) => set({ percent })} label="Percent of calls" suffix="% of calls, picked at random" />}
      </div>
    </Field>
  );
}

/** How long it lasts, and optionally how many hits it ends after. */
export function LifetimeFields({ form, set, problems }: { form: RuleForm; set: Set; problems: LabProblem[] }) {
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <Field id="rule-ends" label="Lasts" field="ends_in_secs" problems={problems} help="It ends by itself. Stop all ends it sooner.">
        <div className="flex items-center gap-2">
          <Input id="rule-ends" inputMode="numeric" value={form.ends} onChange={(e) => set({ ends: e.target.value })} aria-invalid={problems.some((p) => p.field === "ends_in_secs")} className="h-9 w-24 bg-card font-mono" data-testid="rule-ends" />
          <Segmented label="Unit" value={form.unit} onChange={(unit: Unit) => set({ unit })} options={[{ value: "seconds", label: "sec" }, { value: "minutes", label: "min" }, { value: "hours", label: "hours" }]} testId="rule-unit" />
        </div>
      </Field>
      <Field id="rule-max-hits" label="Or after this many hits" field="max_hits" problems={problems} help="Optional. It ends at whichever comes first.">
        <NumberField id="rule-max-hits" value={form.max_hits} onChange={(max_hits) => set({ max_hits })} label="Ends after this many hits" />
      </Field>
    </div>
  );
}
