// Step 2 of the rule wizard (eventbridge__rule-create-pattern): the sample event, the pattern, and a test of one against the other (TestEventPattern).
import { CheckIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormSection, RuleHelp, useKindOp } from "../../../kit";
import { RULES } from "./api";
import { JsonBox } from "./json-box";
import { SAMPLE_AWS, SAMPLE_OWN, type SampleKind, formOf, patternWithForm, testWords } from "./pattern-model";
import { patternProblem } from "./rule-model";
import type { TestAnswer } from "./types";

export function RulePatternStep({ pattern, onPattern, tried }: { pattern: string; onPattern: (p: string) => void; tried: boolean }) {
  const [kind, setKind] = useState<SampleKind>("own");
  const [sample, setSample] = useState(SAMPLE_OWN);
  const [form, setForm] = useState(false);
  const test = useKindOp<TestAnswer>(RULES, "TestEventPattern", { keep: true });
  const problem = patternProblem(pattern);
  const failed = test.error instanceof ApiFailure ? test.error : null;
  const choose = (k: SampleKind) => {
    setKind(k);
    if (k === "aws") setSample(SAMPLE_AWS);
    test.reset();
  };
  return (
    <>
      <FormSection title="Event source" testId="section-source">
        <Field label="Event source">{(id) => (
          <Select value="aws">
            <SelectTrigger id={id} className="w-full bg-card" data-testid="event-source"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem value="aws">AWS events or EventBridge partner events</SelectItem></SelectContent>
          </Select>
        )}</Field>
      </FormSection>
      <FormSection title="Sample events" description="An event to test the pattern against. It is only used here; it is not saved with the rule." testId="section-sample">
        <div className="grid gap-1.5">
          <Label>Sample event type</Label>
          <RadioGroup value={kind} onValueChange={(v) => choose(v as SampleKind)} className="grid gap-3 sm:grid-cols-3" aria-label="Sample event type">
            {([["aws", "AWS events", false], ["partner", "EventBridge partner events", true], ["own", "Enter my own", false]] as const).map(([v, title, off]) => (
              <Label key={v} htmlFor={`sample-${v}`} className={`flex items-center gap-3 rounded-xl border bg-card p-4 font-normal has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40 ${off ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`} title={off ? "doze-aws refuses partner event sources: there is no partner to send them." : undefined}>
                <RadioGroupItem id={`sample-${v}`} value={v} disabled={off} data-testid={`sample-${v}`} />
                <span className="font-medium text-bright">{title}</span>
              </Label>
            ))}
          </RadioGroup>
        </div>
        <JsonBox value={sample} onChange={(s) => { setSample(s); test.reset(); }} label="Sample event" rows={13} testId="sample-event" />
        <RuleHelp>The test needs a whole event: <span className="font-mono">id</span>, <span className="font-mono">account</span>, <span className="font-mono">source</span>, <span className="font-mono">time</span>, <span className="font-mono">region</span>, <span className="font-mono">resources</span> and <span className="font-mono">detail-type</span>. Copy an AWS sample, switch to Enter my own, and paste it to start from a real shape. EventBridge partner events are not available in doze-aws: it refuses partner event sources.</RuleHelp>
      </FormSection>
      <FormSection title="Creation method" testId="section-method">
        <Field label="Method" hint="Use schema is not available in doze-aws: there is no Schema registry to build a pattern from.">{(id) => (
          <Select value="custom">
            <SelectTrigger id={id} className="w-full bg-card" data-testid="creation-method"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem value="schema" disabled>Use schema (not available in doze-aws)</SelectItem><SelectItem value="custom">Custom pattern (JSON editor)</SelectItem></SelectContent>
          </Select>
        )}</Field>
      </FormSection>
      <FormSection title="Event pattern" description="An event matches when every field you list matches. Each field's value is a list; one entry of the list matching is enough." testId="section-pattern">
        <JsonBox value={pattern} onChange={(p) => { onPattern(p); test.reset(); }} label="Event pattern" rows={9} testId="event-pattern" extra={<><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setForm(true)} data-testid="pattern-form-open">Event pattern form</Button><Button type="button" size="sm" variant="outline" className="ml-auto bg-card" disabled={problem !== null || test.isPending} onClick={() => test.mutate({ EventPattern: pattern, Event: sample } as never)} data-testid="test-pattern">Test pattern</Button></>} />
        {tried && problem && <p className="text-[13px] text-bad" role="alert" data-testid="pattern-problem">{problem}</p>}
        {test.data && <div className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-[14px] ${test.data.Result ? "border-good/30 bg-good/10" : "border-bad/30 bg-bad/10"}`} role="status" data-testid="test-result" data-match={test.data.Result ? "yes" : "no"}>{test.data.Result ? <CheckIcon aria-hidden className="mt-0.5 size-[18px] text-good" /> : <XIcon aria-hidden className="mt-0.5 size-[18px] text-bad" />}<p><b className="font-semibold text-bright">{testWords(Boolean(test.data.Result), pattern).split(". ")[0]}.</b> {testWords(Boolean(test.data.Result), pattern).split(". ").slice(1).join(". ")}</p></div>}
        {failed && <Alert tone="bad" title={`TestEventPattern answered ${failed.code}`} testId="test-error">{failed.message}</Alert>}
        <RuleHelp>A pattern can be at most <b>2,048</b> characters by default; the quota can be raised. Numeric conditions compare values as 64-bit floating-point numbers.</RuleHelp>
      </FormSection>
      <PatternForm open={form} onOpenChange={setForm} pattern={pattern} onApply={onPattern} />
    </>
  );
}

function PatternForm({ open, onOpenChange, pattern, onApply }: { open: boolean; onOpenChange: (o: boolean) => void; pattern: string; onApply: (p: string) => void }) {
  const now = formOf(pattern);
  const [source, setSource] = useState<string | null>(null);
  const [type, setType] = useState<string | null>(null);
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) { setSource(null); setType(null); } }}>
      <DialogContent className="sm:max-w-[520px]" data-testid="pattern-form">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Event pattern form</DialogTitle><DialogDescription>Name the source and the detail type, one or more, separated by commas. The rest of the pattern stays as it is.</DialogDescription></DialogHeader>
        <Field label="Source">{(id) => <Input id={id} value={source ?? now.source} onChange={(e) => setSource(e.target.value)} className="font-mono" data-testid="form-source" />}</Field>
        <Field label="Detail type">{(id) => <Input id={id} value={type ?? now.detailType} onChange={(e) => setType(e.target.value)} className="font-mono" data-testid="form-detail-type" />}</Field>
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="button" onClick={() => { onApply(patternWithForm(pattern, source ?? now.source, type ?? now.detailType)); onOpenChange(false); }} data-testid="form-apply">Apply to the pattern</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
