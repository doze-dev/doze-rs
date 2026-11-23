// How the gallery lays a component out: a section per component, a labelled specimen per state. Gallery-only (not shared).
import { cn } from "@doze/ui/lib/utils";

/** One component (or one family): its name, the file it lives in, a sentence, then its specimens. */
export function Section({ title, source, children, note, id }: { title: string; /** The file under packages/ui/src. */ source: string; children: React.ReactNode; note?: React.ReactNode; id?: string }) {
  return (
    <section className="border-t py-8 first:border-t-0 first:pt-0" aria-labelledby={id ? `${id}-title` : undefined} data-testid={id ? `section-${id}` : undefined}>
      <div className="mb-5 flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <h2 id={id ? `${id}-title` : undefined} className="text-[17px] font-semibold tracking-[-0.01em] text-bright">
          {title}
        </h2>
        <code className="text-[12px] text-muted-foreground">{source}</code>
      </div>
      {note && <p className="-mt-2 mb-5 max-w-[46rem] text-[13.5px] text-muted-foreground">{note}</p>}
      <div className="flex flex-col gap-5">{children}</div>
    </section>
  );
}

/** One state of a component, named on the left (`Disabled`, `Long text`). */
export function Specimen({ label, children, className, stack = false }: { label: string; children: React.ReactNode; className?: string; /** The specimen under its label (a wide one: a table, a strip). */ stack?: boolean }) {
  return (
    <div className={cn("grid items-start gap-x-6 gap-y-2", stack ? "grid-cols-1" : "grid-cols-1 md:grid-cols-[9.5rem_minmax(0,1fr)]")}>
      <div className={cn("text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase", !stack && "md:pt-2")}>{label}</div>
      <div className={cn("flex min-w-0 flex-wrap items-center gap-3", className)}>{children}</div>
    </div>
  );
}
