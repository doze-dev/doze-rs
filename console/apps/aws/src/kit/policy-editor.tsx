import { useMemo } from "react";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { Badge } from "@doze/ui/components/ui/badge";
import { cn } from "@doze/ui/lib/utils";
import { checkPolicy, type PolicyKind, statementWords } from "./policy";

/** An IAM policy typed as JSON, with what it says in words beside it: the JSON editor (checked as it is typed, Format), the
 * problems with the shape of the policy (a missing Effect, a Principal where there must be none), and a list of the statements
 * as sentences, so a person sees what they wrote, not only that it parses. IAM validates the rest when the policy is put. */
export function PolicyEditor({ value, onChange, kind = "identity", label = "Policy document", maxSize, help, rows = 14, className, testId = "policy-editor" }: { value: string; onChange: (v: string) => void; kind?: PolicyKind; label?: string; maxSize?: number; help?: React.ReactNode; rows?: number; className?: string; testId?: string }) {
  const check = useMemo(() => checkPolicy(value, kind, maxSize), [value, kind, maxSize]);
  const parses = check.statements.length > 0 || check.problems.every((p) => !p.startsWith("It is not JSON") && p !== "The policy is empty.");
  return (
    <div className={cn("grid gap-4", className)} data-slot="policy-editor" data-testid={testId}>
      <JsonEditor value={value} onChange={onChange} label={label} help={help} rows={rows} testId={`${testId}-json`} />
      {parses && check.problems.length > 0 && (
        <ul className="grid gap-1 rounded-xl border border-warn/30 bg-warn/10 px-4 py-3 text-[13.5px] text-foreground" data-testid={`${testId}-problems`}>
          {check.problems.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      )}
      {check.statements.length > 0 && (
        <section aria-label="Statements" data-testid={`${testId}-statements`}>
          <h3 className="mb-2 text-[13px] font-medium tracking-[0.04em] text-muted-foreground uppercase">What it says</h3>
          <ol className="grid gap-2">
            {check.statements.map((s) => (
              <li key={s.index} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 rounded-lg border bg-well px-3.5 py-2.5 text-[13.5px]" data-testid={`${testId}-statement`}>
                <Badge variant="outline" className={cn("font-medium", s.effect === "Allow" ? "border-good/40 bg-good/10 text-good" : s.effect === "Deny" ? "border-bad/40 bg-bad/10 text-bad" : "text-muted-foreground")}>
                  {s.effect}
                </Badge>
                <span className="min-w-0 flex-1 font-mono text-[12.5px] break-all text-foreground">{statementWords(s).slice(s.effect.length + 1)}</span>
                {s.sid && <span className="text-[12px] text-muted-foreground">{s.sid}</span>}
              </li>
            ))}
          </ol>
        </section>
      )}
    </div>
  );
}
