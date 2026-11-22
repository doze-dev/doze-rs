// The JSON editor of a policy (iam__policy-create, role-trust-edit, policy-create-error): the text, what is wrong with it said before any call
// ("No syntax errors", or "N errors" with each), and the note that IAM Access Analyzer's findings are not here.
import { CircleAlertIcon, CircleCheckIcon } from "lucide-react";
import { useMemo } from "react";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { cn } from "@doze/ui/lib/utils";
import { checkPolicy, type PolicyKind } from "../../../kit";

/** What a policy text has wrong with it, one sentence each: not JSON, or the shape (an Effect, an Action, a Resource or a Principal). */
export function policyErrors(text: string, kind: PolicyKind | "trust"): string[] {
  const c = checkPolicy(text, kind === "trust" ? "resource" : kind);
  const problems = c.problems.filter((p) => p !== "The policy is empty.").map((p) => (kind === "identity" && / needs a Resource\.$/.test(p) ? "Policy statement must contain resources." : p));
  if (kind === "trust" && c.statements.length > 0) {
    try {
      const doc = JSON.parse(text) as { Statement?: unknown };
      const list = Array.isArray(doc.Statement) ? doc.Statement : [doc.Statement];
      list.forEach((s, i) => {
        const st = (s ?? {}) as Record<string, unknown>;
        if (st.Resource !== undefined || st.NotResource !== undefined) problems.push(`Statement ${i + 1} names a Resource, which a trust policy cannot have.`);
      });
    } catch {
      // not JSON: said already
    }
  }
  return problems;
}

export function PolicyJson({ value, onChange, kind, label = "Policy editor", rows = 16, extra, editor, testId = "policy-json" }: { value: string; onChange: (v: string) => void; kind: PolicyKind | "trust"; label?: string; rows?: number; /** Under the status line: the refusal that was answered, or a suggestion. */ extra?: React.ReactNode; /** Another editor of the same document (the Visual one) in the place of the JSON text. */ editor?: React.ReactNode; testId?: string }) {
  const errors = useMemo(() => policyErrors(value, kind), [value, kind]);
  const parses = !errors.some((e) => e.startsWith("It is not JSON"));
  const ok = errors.length === 0 && value.trim() !== "";
  return (
    <div className="grid gap-3" data-testid={testId}>
      {editor ?? <JsonEditor value={value} onChange={onChange} label={label} rows={rows} testId={`${testId}-editor`} highlight />}
      <p className={cn("flex items-center gap-2 text-[13.5px] font-medium", ok ? "text-good" : "text-bad")} data-testid={`${testId}-status`}>
        {ok ? <CircleCheckIcon aria-hidden className="size-4" /> : <CircleAlertIcon aria-hidden className="size-4" />}
        {ok ? "No syntax errors" : parses ? `${errors.length} ${errors.length === 1 ? "error" : "errors"}` : "1 error"}
      </p>
      {!ok && parses && errors.length > 0 && (
        <ul className="grid gap-1 text-[13.5px] text-foreground" data-testid={`${testId}-errors`}>
          {errors.map((e) => <li key={e} className="border-l-2 border-bad pl-3">{e}</li>)}
        </ul>
      )}
      {extra}
      <p className="text-[13px] leading-5 text-muted-foreground" data-testid={`${testId}-analyzer`}>Security warnings, general warnings and suggestions come from IAM Access Analyzer, which doze-aws does not run.</p>
    </div>
  );
}
