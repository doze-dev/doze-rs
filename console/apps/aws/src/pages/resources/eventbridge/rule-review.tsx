// Step 5 of the rule wizard (eventbridge__rule-create-review): each step's answers with an Edit back to it. The calls it makes are beside it.
import { ReviewSection } from "@doze/ui/components/app/stepper";
import { filled } from "@doze/ui/components/app/key-values";
import { Badge } from "@doze/ui/components/ui/badge";
import type { RuleDraft } from "./rule-model";
import { targetsOf } from "./rule-model";
import { cronWords, parseCron } from "./cron";
import { arnName, inputWords, prettyPattern, retryWords, targetWords } from "./words";

const mono = (t: string) => <span className="font-mono">{t}</span>;

export function RuleReview({ d, onEdit }: { d: RuleDraft; onEdit: (step: number) => void }) {
  const schedule = d.kind === "schedule";
  const tags = filled(d.tags);
  const cron = parseCron(d.schedule);
  const targets = targetsOf(d);
  return (
    <>
      <ReviewSection title={schedule ? "Step 1: Define rule detail and schedule" : "Step 1: Define rule detail"} onEdit={() => onEdit(0)} rows={[
        { label: "Name", value: mono(d.name) }, { label: "Description", value: d.description || <span className="text-muted-foreground">None</span> }, { label: "Event bus", value: mono(d.bus) },
        schedule ? { label: "Schedule", value: <>{mono(d.schedule)}{cron ? <span className="text-muted-foreground"> {cronWords(cron)}</span> : null}</> } : { label: "Rule type", value: "Rule with an event pattern" },
        ...(schedule ? [{ label: "State", value: d.enabled ? "Enabled" : "Disabled" }] : []),
      ]} />
      {!schedule && <ReviewSection title="Step 2: Build event pattern" onEdit={() => onEdit(1)} rows={[{ label: "Event pattern", value: <pre className="overflow-auto rounded-[var(--radius-field)] bg-well px-4 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap" data-testid="review-pattern">{prettyPattern(d.pattern)}</pre> }]} />}
      <ReviewSection title={`Step ${schedule ? 2 : 3}: Select target(s)`} onEdit={() => onEdit(schedule ? 1 : 2)} rows={d.targets.flatMap((t, i) => [
        { label: d.targets.length > 1 ? `Target ${i + 1}` : "Target", value: t.arn ? targetWords(t.arn) : "–" },
        { label: "Configure target input", value: <>{inputWords(targets[i]!)}{t.input === "transformer" && t.paths ? "" : ""}</> },
        { label: "Retry policy", value: t.retry ? retryWords(targets[i]!) : "Default (24 hours, up to 185 attempts)" },
        { label: "Dead-letter queue", value: t.dlq ? mono(arnName(t.dlq)) : "None" },
      ])} />
      <ReviewSection title={`Step ${schedule ? 3 : 4}: Configure tags`} onEdit={() => onEdit(schedule ? 2 : 3)} rows={[{ label: "Tags", value: tags.length === 0 ? <span className="text-muted-foreground">None</span> : <span className="flex flex-wrap gap-1.5">{tags.map((t) => <Badge key={t.id} variant="outline" className="font-mono text-[11.5px] font-normal">{t.value ? `${t.key}=${t.value}` : t.key}</Badge>)}</span> }]} />
    </>
  );
}
