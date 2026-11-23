import { CheckIcon, PencilIcon } from "lucide-react";
import { Fragment } from "react";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { canGoTo, stepStates } from "./stepper-state";

export type Step = { id: string; label: string; /** Shown as `(optional)` after the label. */ optional?: boolean };

/** A wizard's steps in a row: a mark (a tick when done, its number when not), its name, and a line to the next. A step that
 * was passed (or reached before) is a button that goes back to it; the current one says so (`aria-current="step"`); one not
 * yet reached is only a label. `furthest` is the furthest step reached, so a person who went back can come forward again. */
export function Stepper({ steps, current, furthest = current, onStep, className }: { steps: Step[]; current: number; furthest?: number; onStep?: (index: number) => void; className?: string }) {
  const states = stepStates(steps.length, current);
  return (
    <nav aria-label="Steps" className={cn("min-w-0", className)} data-slot="stepper">
      <ol className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {steps.map((s, i) => {
          const state = states[i]!;
          const go = onStep && canGoTo(i, current, furthest);
          const reached = i <= Math.max(furthest, current);
          const mark = (
            <span aria-hidden className={cn("flex size-6 shrink-0 items-center justify-center rounded-full border text-[12px] font-semibold tabular-nums transition-colors", state === "current" ? "border-primary bg-primary text-primary-foreground" : reached ? "border-good text-good" : "text-muted-foreground")}>
              {state === "current" ? i + 1 : reached && state === "done" ? <CheckIcon className="size-3.5" /> : i + 1}
            </span>
          );
          const label = (
            <span className={cn("text-[14px] whitespace-nowrap", state === "current" ? "font-medium text-bright" : "text-muted-foreground")}>
              {s.label}
              {s.optional ? " (optional)" : ""}
            </span>
          );
          return (
            <Fragment key={s.id}>
              {i > 0 && <li aria-hidden className="h-px w-8 bg-border" />}
              <li data-step={s.id} data-state={state}>
                {go ? (
                  <button type="button" onClick={() => onStep(i)} className="flex items-center gap-2 rounded-full outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 [&:hover_span:last-child]:text-foreground" aria-label={`Go to step ${i + 1}: ${s.label}`}>
                    {mark}
                    {label}
                  </button>
                ) : (
                  <span className="flex items-center gap-2" aria-current={state === "current" ? "step" : undefined}>
                    {mark}
                    {label}
                  </span>
                )}
              </li>
            </Fragment>
          );
        })}
      </ol>
    </nav>
  );
}

export type ReviewRow = { label: string; value: React.ReactNode };

/** One step's answers on the review page: its title, what was chosen, and an Edit link back to the step. */
export function ReviewSection({ title, rows, onEdit, className }: { title: string; rows: ReviewRow[]; onEdit?: () => void; className?: string }) {
  return (
    <section className={cn("min-w-0 rounded-[14px] border bg-card p-6 lg:p-7", className)} aria-label={title} data-slot="review-section">
      <div className="flex items-start justify-between gap-3">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
        {onEdit && (
          <Button type="button" variant="ghost" size="sm" onClick={onEdit} aria-label={`Edit ${title}`} data-testid={`review-edit-${title}`}>
            <PencilIcon /> Edit
          </Button>
        )}
      </div>
      <dl className="mt-3 grid grid-cols-[minmax(8rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
        {rows.map((r) => (
          <div key={r.label} className="contents">
            <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{r.label}</dt>
            <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright">{r.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
