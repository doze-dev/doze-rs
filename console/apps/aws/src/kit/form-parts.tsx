import { ChevronDownIcon, ChevronRightIcon, CircleAlertIcon } from "lucide-react";
import { useId, useState } from "react";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Collapsible, CollapsibleContent } from "@doze/ui/components/ui/collapsible";
import { Label } from "@doze/ui/components/ui/label";
import { cn } from "@doze/ui/lib/utils";

/** The service said no to this field: why, and what would work. It is the marker beside the field (a red edge on the control, set by
 * {@link Field}) and the words under it; it names the problem the service named, not a guess of the console's. */
export function FieldRefusal({ why, suggestion, id, className }: { why: React.ReactNode; suggestion?: React.ReactNode; id?: string; className?: string }) {
  return (
    <div id={id} role="alert" className={cn("flex gap-2 text-[13px] leading-5 text-bad", className)} data-slot="field-refusal">
      <CircleAlertIcon aria-hidden className="mt-0.5 size-4 shrink-0" />
      <div className="min-w-0">
        <span>{why}</span>
        {suggestion && <span className="text-foreground"> {suggestion}</span>}
      </div>
    </div>
  );
}

/** The rule of a field in the service's own numbers, set apart by a rule at the left (`Range 0 seconds to 12 hours, default 30
 * seconds.`). Bold what is a limit: pass `<b>`. */
export function RuleHelp({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground [&_b]:font-semibold [&_b]:text-foreground", className)} data-slot="rule-help">
      {children}
    </div>
  );
}

/** A label, the control, the rule (`help`), a hint drawn from what is really there (`hint`: `pick-lists already exists in this
 * Region.`), and the service's refusal of this field. `children` is the control, or a function of its id. */
export function Field({ label, optional = false, help, hint, error, refusal, children, className, testId }: { label: React.ReactNode; /** The word `optional` after the label. */ optional?: boolean; help?: React.ReactNode; hint?: React.ReactNode; /** A problem the form found itself. */ error?: string; refusal?: { why: React.ReactNode; suggestion?: React.ReactNode } | null; children: React.ReactNode | ((id: string) => React.ReactNode); className?: string; testId?: string }) {
  const id = useId();
  const refused = Boolean(refusal) || Boolean(error);
  return (
    <div className={cn("grid min-w-0 gap-1.5", className)} data-slot="field" data-refused={refused ? "" : undefined} data-testid={testId}>
      <Label htmlFor={id}>{label}{optional && <span className="font-normal text-muted-foreground">optional</span>}</Label>
      <div className="min-w-0 data-[refused]:[&_input]:border-bad data-[refused]:[&_textarea]:border-bad data-[refused]:[&_[role=combobox]]:border-bad" data-refused={refused ? "" : undefined}>
        {typeof children === "function" ? children(id) : children}
      </div>
      {error && !refusal && <FieldRefusal why={error} id={`${id}-error`} />}
      {refusal && <FieldRefusal why={refusal.why} suggestion={refusal.suggestion} id={`${id}-refusal`} />}
      {help && <RuleHelp>{help}</RuleHelp>}
      {hint && <p className="text-[13px] leading-5 text-muted-foreground" data-slot="field-hint">{hint}</p>}
    </div>
  );
}

/** After a submit that found problems: brings the first field they are said on into view and puts the cursor in it (a person who
 * pressed the button at the foot of a long form would otherwise see only a grey button). Does nothing when no field is marked. */
export function focusFirstProblem(form: HTMLElement | null): void {
  const field = form?.querySelector<HTMLElement>("[data-slot=field][data-refused]");
  if (!field) return;
  field.scrollIntoView({ block: "center", behavior: "auto" });
  field.querySelector<HTMLElement>("input, textarea, [role=combobox]")?.focus({ preventScroll: true });
}

/** A card of a form: a title, `Optional` when it is, a sentence, and its fields. A collapsible one opens and closes (`Expand`
 * and `Collapse`), and says in `summary` what it holds while shut (`Disabled.`, `No tags.`): a closed section is never blank. */
export function FormSection({ title, optional = false, description, collapsible = false, defaultOpen = true, summary, actions, children, className, testId }: { title: string; optional?: boolean; description?: React.ReactNode; /** At the right of the title (the section's own button, an Edit). */ actions?: React.ReactNode; collapsible?: boolean; defaultOpen?: boolean; summary?: React.ReactNode; children: React.ReactNode; className?: string; testId?: string }) {
  const [open, setOpen] = useState(defaultOpen || !collapsible);
  const id = useId();
  return (
    <section className={cn("min-w-0 rounded-[14px] border bg-card p-6 lg:p-7", className)} aria-labelledby={`${id}-title`} data-slot="form-section" data-testid={testId ?? `section-${title}`}>
      <Collapsible open={open} onOpenChange={setOpen}>
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 id={`${id}-title`} className="flex flex-wrap items-center gap-2.5 text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
              {title}
              {optional && <Badge variant="secondary" className="font-normal text-muted-foreground">Optional</Badge>}
            </h2>
            {(open || !summary) && description && <p className="mt-1 text-[13.5px] text-muted-foreground">{description}</p>}
            {!open && summary && <div className="mt-1 text-[13.5px] text-muted-foreground" data-testid={`${testId ?? `section-${title}`}-summary`}>{summary}</div>}
          </div>
          {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
          {collapsible && (
            <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setOpen(!open)} aria-expanded={open} data-testid={`${testId ?? `section-${title}`}-toggle`}>
              {open ? <ChevronDownIcon /> : <ChevronRightIcon />} {open ? "Collapse" : "Expand"}
            </Button>
          )}
        </div>
        <CollapsibleContent className="data-[state=open]:animate-in data-[state=open]:fade-in-0 motion-reduce:animate-none">
          <div className="mt-5 grid gap-5">{children}</div>
        </CollapsibleContent>
      </Collapsible>
    </section>
  );
}
