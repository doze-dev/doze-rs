// Configure input transformer (eventbridge__rule-transformer): a sample event, the input paths and the template, and the output they make.
import { useState } from "react";
import { checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { RuleHelp, Segmented } from "../../../kit";
import { JsonBox } from "./json-box";
import { SAMPLE_AWS, SAMPLE_OWN } from "./pattern-model";
import { transform } from "./transform";
import { prettyJson } from "./words";

export const EXAMPLE_PATHS = '{\n  "order_id": "$.detail.order_id",\n  "customer": "$.detail.customer_id"\n}';
export const EXAMPLE_TEMPLATE = '{\n  "order_id": "<order_id>",\n  "customer": "<customer>"\n}';

/** What the paths and the template make of `sample`: the output, or why there is none. */
export function generate(sample: string, paths: string, template: string): { output: string } | { problem: string } {
  const e = checkJson(sample, "invalid");
  if (!e.ok) return { problem: `The sample event is not JSON: ${e.why}` };
  const p = checkJson(paths === "" ? "{}" : paths, "invalid");
  if (!p.ok || typeof p.value !== "object" || p.value === null || Array.isArray(p.value)) return { problem: "The input paths are a JSON object of names and paths." };
  const out = transform(e.value, p.value as Record<string, string>, template);
  return "output" in out ? { output: prettyJson(out.output) } : out;
}

export function TransformerDialog({ open, onOpenChange, paths, template, target, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; paths: string; template: string; target: string; onConfirm: (paths: string, template: string) => void }) {
  const [p, setP] = useState(paths || EXAMPLE_PATHS);
  const [t, setT] = useState(template || EXAMPLE_TEMPLATE);
  const [sample, setSample] = useState(SAMPLE_OWN);
  const [kind, setKind] = useState<"aws" | "own">("own");
  const [out, setOut] = useState<ReturnType<typeof generate> | null>(null);
  const names = Object.keys((checkJson(p, "ok") as { value?: Record<string, string> }).value ?? {}).length;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[92vh] overflow-y-auto sm:max-w-[760px]" data-testid="transformer-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Configure input transformer</DialogTitle>
          <DialogDescription>Shape what <span className="font-mono font-semibold text-bright">{target || "the target"}</span> receives from the matched event.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-1.5">
          <Label>Sample event</Label>
          <Segmented<"aws" | "own"> label="Sample event type" value={kind} onChange={(k) => { setKind(k); setSample(k === "aws" ? SAMPLE_AWS : SAMPLE_OWN); setOut(null); }} options={[{ value: "aws", label: "AWS events" }, { value: "own", label: "Enter your own" }]} className="w-fit" testId="transformer-kind" />
          <p className="text-[12.5px] text-muted-foreground">EventBridge partner events are not available in doze-aws.</p>
        </div>
        <JsonBox value={sample} onChange={(v) => { setSample(v); setOut(null); }} label="Sample event" hideLabel rows={8} testId="transformer-sample" />
        <JsonBox value={p} onChange={(v) => { setP(v); setOut(null); }} label="Input path" rows={5} testId="transformer-paths" />
        <div className="grid gap-1.5">
          <Label htmlFor="transformer-template">Template</Label>
          <Textarea id="transformer-template" value={t} onChange={(e) => { setT(e.target.value); setOut(null); }} rows={6} spellCheck={false} className="bg-card font-mono text-[13px] leading-5" data-testid="transformer-template" />
        </div>
        <RuleHelp>Up to <b>100</b> variables; the template can be <b>8,192</b> characters. Paths use dot notation. A variable whose path isn't in the event is left out of the output (an empty text inside quotes, <span className="font-mono">null</span> outside them).</RuleHelp>
        <div className="flex items-center gap-3">
          <Button type="button" variant="outline" className="bg-card" onClick={() => setOut(generate(sample, p, t))} data-testid="transformer-generate">Generate output</Button>
          <span className="text-[13px] text-muted-foreground">{names} {names === 1 ? "variable" : "variables"}</span>
        </div>
        {out && ("output" in out ? (
          <div className="grid gap-1.5" data-testid="transformer-output"><Label>Output</Label><pre className="overflow-auto rounded-[var(--radius-field)] bg-well px-4 py-3 font-mono text-[13px] leading-5">{out.output}</pre></div>
        ) : (
          <p className="rounded-[var(--radius-field)] border border-bad/30 bg-bad/5 px-4 py-3 text-[13px] text-bad" role="alert" data-testid="transformer-problem">{out.problem}</p>
        ))}
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="button" onClick={() => { onConfirm(p, t); onOpenChange(false); }} data-testid="transformer-confirm">Confirm</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
