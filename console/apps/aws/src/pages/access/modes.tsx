import { cn } from "@doze/ui/lib/utils";
import { CodeBlock } from "../card";

const MODES = [
  { mode: "off", title: "Off", words: "No request is checked against a policy. Nothing is logged." },
  { mode: "soft", title: "Soft", words: "Every request is checked and IAM logs what it would refuse, then lets it through. Live marks those calls would be denied." },
  { mode: "enforce", title: "Enforce", words: "A request the caller's policies do not allow is refused with AccessDenied, in AWS's words, as AWS would." },
] as const;

/** The IAM mode: the three, the one this doze-aws runs, and how to change it (a setting read at start: doze-aws restarts). */
export function Modes({ mode }: { mode: string }) {
  return (
    <section className="mb-10 rounded-[14px] border bg-card p-6 lg:p-7" aria-labelledby="iam-mode-title" data-testid="iam-mode" data-mode={mode}>
      <h2 id="iam-mode-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
        IAM mode
      </h2>
      <p className="mt-1.5 text-[13.5px] text-muted-foreground">Keys IAM issued (AKIA…) and role sessions are checked; any other key is the account root, allowed everything.</p>
      <div className="mt-5 grid gap-4 md:grid-cols-3" role="list">
        {MODES.map((m) => (
          <div key={m.mode} role="listitem" className={cn("rounded-xl border px-5 py-4", m.mode === mode ? "border-primary bg-accent/40 ring-2 ring-primary/15" : "bg-card")} data-testid={`mode-${m.mode}`} aria-current={m.mode === mode ? "true" : undefined}>
            <div className="flex items-center justify-between">
              <span className="font-semibold text-bright">{m.title}</span>
              {m.mode === mode && <span className="rounded-md bg-primary px-1.5 text-[11px] font-semibold tracking-[0.05em] text-primary-foreground uppercase">This run</span>}
            </div>
            <p className="mt-1.5 text-[13px] leading-5 text-foreground">{m.words}</p>
          </div>
        ))}
      </div>
      <div className="mt-6 grid gap-2">
        <p className="text-[13.5px] text-foreground">The mode is read at start. To change it, start doze-aws again with the flag, its variable, or the line in doze-aws.toml:</p>
        <CodeBlock code={"doze-aws --iam enforce\n# or\nDOZE_AWS_IAM=enforce doze-aws\n# or, in doze-aws.toml\niam = \"enforce\""} what="Command copied" testId="iam-change" />
      </div>
    </section>
  );
}
