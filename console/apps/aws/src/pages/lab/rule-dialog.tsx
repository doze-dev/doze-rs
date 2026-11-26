import { FlaskConicalIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { cn } from "@doze/ui/lib/utils";
import { useLabCheck, useLabGuide, useMakeRule } from "../../api/lab";
import { useLab } from "../../api/spaces";
import type { LabProblem } from "../../api/types/LabProblem";
import { useDebounced } from "../../kit/debounce";
import { useRegions } from "../../shell/region-menu";
import { RuleLesson } from "./lesson";
import { ErrorFields, Field, LifetimeFields, MatchFields, ShareFields, SlowFields } from "./rule-fields";
import { CUSTOM_WORDS, RULE_TABS, type RuleForm, type RuleTab, toBody } from "./rule-form";

/** The kind of rule, one of the guide's six: a group of buttons, one pressed. */
function KindPicker({ tab, onPick, titles }: { tab: RuleTab; onPick: (t: RuleTab) => void; titles: Record<string, string> }) {
  return (
    <div role="radiogroup" aria-label="What the rule does" className="flex flex-wrap gap-2" data-testid="rule-kind">
      {RULE_TABS.map((t) => (
        <button key={t} type="button" role="radio" aria-checked={t === tab} onClick={() => onPick(t)} className={cn("rounded-md border px-3 py-1.5 text-[13px] transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", t === tab ? "border-primary bg-accent font-medium text-bright" : "bg-card text-muted-foreground hover:text-foreground")} data-testid={`rule-kind-${t}`}>
          {titles[t] ?? t}
        </button>
      ))}
    </div>
  );
}

/** The sentence the rule would be, from the server, and what is wrong with it that no field owns. It is the same sentence the table shows. */
function Sentence({ sentence, problems, answers, pending }: { sentence: string; problems: LabProblem[]; answers: { service: string; status: number; code: string }[]; pending: boolean }) {
  const loose = problems.filter((p) => p.field === "rules" || p.field === "name");
  // The fields say their own problems; the footer, which stays in view, repeats them (for the eye only) so a problem below the fold is not missed.
  const owned = problems.filter((p) => p.field !== "rules" && p.field !== "name");
  return (
    <div className="grid gap-1" aria-live="polite" data-testid="rule-sentence" data-pending={pending ? "" : undefined}>
      <p className={cn("text-[14px] font-medium text-bright transition-opacity", pending && "opacity-60")} data-testid="rule-sentence-text">{sentence || "Say which calls and what happens to them."}</p>
      {answers.length > 0 && <p className="text-[12.5px] text-muted-foreground">{answers.slice(0, 4).map((a) => `${a.service} answers ${a.status} ${a.code}`).join("; ")}{answers.length > 4 ? `; and ${answers.length - 4} more` : ""}.</p>}
      {loose.map((p) => <p key={p.text} className="text-[12.5px] text-destructive" role="alert" data-testid={`problem-${p.field}`}>{p.text}</p>)}
      {owned.slice(0, 2).map((p) => <p key={p.text} aria-hidden className="text-[12.5px] text-destructive" data-testid="problem-summary">{p.text}</p>)}
    </div>
  );
}

function Body({ initial, onDone }: { initial: RuleForm; onDone: () => void }) {
  const [form, setForm] = useState(initial);
  const [refusal, setRefusal] = useState<{ problems: LabProblem[] } | null>(null);
  const guide = useLabGuide();
  const lab = useLab();
  const regions = useRegions().data?.regions ?? [];
  const make = useMakeRule();
  const body = useMemo(() => toBody(form), [form]);
  const asked = useDebounced(JSON.stringify(body), 250);
  const check = useLabCheck(JSON.parse(asked.value));
  const set = (patch: Partial<RuleForm>) => (setForm((f) => ({ ...f, ...patch })), setRefusal(null));
  const titles = { ...Object.fromEntries((guide.data?.rehearsals ?? []).map((r) => [r.tab, r.title])), custom: CUSTOM_WORDS.title };
  const teach = guide.data?.rehearsals.find((r) => r.tab === form.tab);
  const problems = refusal?.problems ?? check.data?.problems ?? [];
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (make.isPending) return;
    make.mutate(body, {
      onSuccess: (r) => {
        if (r.outcome === "refused") return setRefusal({ problems: r.check.problems });
        toast.success(`Rule made: ${r.rule.name}`);
        onDone();
      },
    });
  };
  const failed = make.error ? errorWords(make.error) : null;
  const ready = check.data?.ok === true && !check.isPlaceholderData && asked.settled && !make.isPending;
  return (
    <form onSubmit={submit} className="grid gap-5" data-testid="rule-form">
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2 text-[21px] font-semibold"><FlaskConicalIcon className="size-5 text-warn" aria-hidden /> Make a rule</DialogTitle>
        <DialogDescription className="text-[14px] leading-[22px]">A rule bends the calls your app sends to doze-aws for a while, and ends by itself. Calls from the console are never bent.</DialogDescription>
      </DialogHeader>
      <KindPicker tab={form.tab} onPick={(tab) => set({ tab })} titles={titles} />
      {teach && (
        <details className="rounded-lg border bg-card px-4 py-3" data-testid="rule-lesson">
          <summary className="cursor-pointer text-[13.5px] font-medium text-bright">{teach.title}: {teach.lede}</summary>
          <div className="mt-3"><RuleLesson teach={teach} /></div>
        </details>
      )}
      <section className="grid gap-3"><h3 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">Which calls</h3>
        <MatchFields form={form} set={set} problems={problems} targets={lab.data?.targets ?? []} regions={regions} />
      </section>
      {form.tab === "custom" && (
        <details className="rounded-lg border bg-card px-4 py-3" data-testid="rule-lesson">
          <summary className="cursor-pointer text-[13.5px] font-medium text-bright">{CUSTOM_WORDS.title}: {CUSTOM_WORDS.lede}</summary>
          <p className="mt-3 text-[13px] leading-5 text-muted-foreground"><b className="font-semibold text-foreground">In production: </b>{CUSTOM_WORDS.production}</p>
        </details>
      )}
      {form.tab === "slow" && <section className="grid gap-3"><h3 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">What happens</h3><SlowFields form={form} set={set} problems={problems} max={60_000} /></section>}
      {form.tab === "custom" && <section className="grid gap-3"><h3 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">What happens</h3><ErrorFields form={form} set={set} problems={problems} /></section>}
      <section className="grid gap-3"><h3 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">How often and how long</h3>
        <ShareFields form={form} set={set} problems={problems} />
        <LifetimeFields form={form} set={set} problems={problems} />
      </section>
      <Field id="rule-name" label="Name" field="name" problems={problems} help="Optional. Left empty, the rule is named after what it does.">
        <Input id="rule-name" value={form.name} maxLength={80} onChange={(e) => set({ name: e.target.value })} placeholder={check.data?.sentence ?? ""} className="h-9 bg-card" data-testid="rule-name" />
      </Field>
      {failed && <p className="text-[13px] text-destructive" role="alert" data-testid="rule-error">{failed.message}</p>}
      <div className="sticky -bottom-6 -mx-6 -mb-6 grid gap-3 border-t bg-background px-6 py-4">
        <Sentence sentence={check.data?.sentence ?? ""} problems={problems} answers={form.tab === "slow" || form.tab === "drop" ? [] : (check.data?.answers ?? [])} pending={!asked.settled || check.isFetching} />
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onDone}>Cancel</Button>
          <Button type="submit" disabled={!ready} data-testid="rule-make">{make.isPending ? "Making…" : "Make rule"}</Button>
        </div>
      </div>
    </form>
  );
}

/** Make a rule: a dialog over the Lab. The kind, the calls it fits, what it does, how often and how long; the sentence it would be and
 * what is wrong with it are the server's (`/rules/check`), asked as the form changes. `initial` is what it opens on (a recipe's calls). */
export function RuleDialog({ initial, onClose }: { initial: RuleForm | null; onClose: () => void }) {
  return (
    <Dialog open={initial !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[780px]" data-testid="rule-dialog">
        {initial && <Body initial={initial} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

