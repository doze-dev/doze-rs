// Step 2 (cloudformation__stack-create-details, stack-update-details): the stack's name, and one field for each parameter the template
// declares, with its type, default and description (GetTemplateSummary). An update starts each at the value the stack has now; a NoEcho
// one starts empty, with Use existing value ticked.
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, FormSection } from "../../../kit";
import { nameList } from "./model";
import { type Declared, type Summary, type Wizard, dot, nameProblem, noEchoLine, parameterProblem, valueOf } from "./wizard-model";

const words = (d: Declared) => `Type ${d.ParameterType ?? "String"}${d.DefaultValue !== undefined && d.DefaultValue !== "" ? `, default ${d.DefaultValue}` : ""}.`;

export function StepDetails({ w, set, summary, taken, tried, current, born }: { w: Wizard; set: (p: Partial<Wizard>) => void; summary: Summary; taken: readonly string[]; tried: boolean; current: Record<string, string>; /** The stack an update is of: when it was created, and whether it has been updated since. */ born?: { created?: string; updated: boolean } }) {
  const update = w.mode !== "create";
  const params = summary.Parameters ?? [];
  const nameError = nameProblem(w.name, taken);
  return (
    <>
      {!update && (
        <FormSection title="Stack name" description="The name is how you find this stack in the list." testId="section-name">
          <Field label="Stack name" error={tried || w.name !== "" ? (nameError ?? undefined) : undefined} testId="field-name"
            help={<>Only letters, digits and hyphens, starting with a letter, up to 128 characters. Names are case sensitive and unique within a Region.</>}
            hint={taken.length > 0 ? <><span className="font-mono text-[12.5px]">{nameList(taken)}</span> already {taken.length === 1 ? "exists" : "exist"} here.</> : undefined}>
            {(id) => <Input id={id} value={w.name} onChange={(e) => set({ name: e.target.value })} className="bg-card font-mono text-[13px]" autoComplete="off" spellCheck={false} data-testid="stack-name" />}
          </Field>
        </FormSection>
      )}
      <FormSection title="Parameters" description={update ? "Values the template asks for. Each starts at the value the stack has now." : "Values the template asks for. Defaults are filled in."} testId="section-parameters">
        {params.length === 0 && <p className="text-[13.5px] text-muted-foreground" data-testid="no-parameters">This template declares no parameters.</p>}
        {params.map((d) => {
          const key = d.ParameterKey;
          const allowed = d.ParameterConstraints?.AllowedValues ?? [];
          const keeping = update && (w.keep[key] ?? false);
          const error = tried ? parameterProblem(w, d) : null;
          const was = current[key];
          const shown = update && !d.NoEcho ? (w.values[key] ?? was ?? d.DefaultValue ?? "") : valueOf(w, d);
          return (
            <Field key={key} label={key} error={error ?? undefined} testId={`param-${key}`} help={update && d.NoEcho ? noEchoLine(w.stack, born?.created, born?.updated ?? false) : undefined} hint={<>{d.Description ? `${dot(d.Description)} ` : ""}{words(d)}</>}>
              {(id) => d.NoEcho ? (
                <div className="grid gap-2">
                  <Input id={id} type="password" value={keeping ? "" : (w.values[key] ?? "")} onChange={(e) => set({ values: { ...w.values, [key]: e.target.value } })} disabled={keeping} placeholder={keeping ? "Hidden, because the parameter is NoEcho" : ""} className="bg-card font-mono text-[13px]" autoComplete="off" data-testid={`param-input-${key}`} />
                  {update && (
                    <label className="flex items-center gap-2 text-[13.5px]"><Checkbox checked={keeping} onCheckedChange={(c) => set({ keep: { ...w.keep, [key]: c === true } })} data-testid={`keep-${key}`} /> Use existing value</label>
                  )}
                </div>
              ) : allowed.length > 0 ? (
                <Select value={shown} onValueChange={(v) => set({ values: { ...w.values, [key]: v } })}>
                  <SelectTrigger id={id} className="w-full bg-card" data-testid={`param-input-${key}`}><SelectValue /></SelectTrigger>
                  <SelectContent>{allowed.map((a) => <SelectItem key={a} value={a}>{a}</SelectItem>)}</SelectContent>
                </Select>
              ) : (
                <Input id={id} value={shown} onChange={(e) => set({ values: { ...w.values, [key]: e.target.value } })} className="bg-card font-mono text-[13px]" autoComplete="off" spellCheck={false} data-testid={`param-input-${key}`} />
              )}
            </Field>
          );
        })}
      </FormSection>
    </>
  );
}
