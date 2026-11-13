// The parts the topic writes share: the call beside a form (`The call`, `The answer`), the teaching under a field, Kafka's
// refusal, a success banner. Text only: every word is the server's, React escapes it.
import { CircleCheckIcon, ChevronDownIcon, NetworkIcon, OctagonXIcon } from "lucide-react";
import { useState } from "react";
import type { Refusal } from "@/api/types/Refusal";
import type { Span } from "@/api/types/Span";
import type { Teach } from "@/api/types/Teach";
import type { Tok } from "@/api/types/Tok";
import { CopyButton } from "@/components/app/copy-button";
import { Spans } from "@/components/app/spans";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { DialogFooter } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

const TOK: Record<Tok, string> = { plain: "text-foreground", keyword: "text-primary", str: "text-good", comment: "text-muted-foreground italic" };

/** A protocol call or answer as the boards draw it: the API and version, then its fields, indented. */
export function CallBlock({ title = "The call", lines, note, className, testId = "the-call" }: { title?: string; lines: [Tok, string][][]; note?: string; className?: string; testId?: string }) {
  return (
    <section className={cn("min-w-0 rounded-xl border bg-card p-4", className)} data-testid={testId} aria-label={title}>
      <h3 className="mb-3 flex items-center gap-2 text-[11.5px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
        <NetworkIcon aria-hidden className="size-3.5" />
        {title}
      </h3>
      <pre className="overflow-x-auto rounded-lg bg-well px-4 py-3 font-mono text-[12.5px] leading-[20px]">
        {lines.map((line, i) => (
          <div key={i} className="min-h-5 whitespace-pre">
            {line.map(([tok, t], j) => (
              <span key={j} className={TOK[tok]}>
                {t}
              </span>
            ))}
          </div>
        ))}
      </pre>
      {note && <p className="mt-3 text-[12.5px] text-muted-foreground">{note}</p>}
    </section>
  );
}

/** A terminal command, with Copy. */
export function Terminal({ title, lines, testId }: { title: string; lines: string[]; testId?: string }) {
  const text = lines.join("\n");
  return (
    <section className="min-w-0 rounded-xl border bg-card p-4" data-testid={testId} aria-label={title}>
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="text-[11.5px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">{title}</h3>
        <CopyButton text={text} what="Command copied" />
      </div>
      <pre className="overflow-x-auto rounded-lg bg-well px-4 py-3 font-mono text-[12.5px] leading-[20px] whitespace-pre">{text}</pre>
    </section>
  );
}

function Line({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <b className="font-semibold text-foreground">{label}</b> {children}
    </div>
  );
}

/** The lines under a field: what Kafka says, the default, the range, when it takes effect, what is so here. */
export function TeachLines({ t, className }: { t: Teach; className?: string }) {
  return (
    <div className={cn("border-l-2 border-border pl-3 text-[12.5px] leading-[19px] text-muted-foreground", className)} data-testid="teach">
      <Line label="Kafka says">{t.says}</Line>
      {t.default.length > 0 && (
        <Line label="Default">
          <Spans spans={t.default} />
        </Line>
      )}
      {t.range.length > 0 && (
        <Line label="Range">
          <Spans spans={t.range} />
        </Line>
      )}
      <Line label="Takes effect">
        <Spans spans={t.takes_effect} />
      </Line>
      {t.here.length > 0 && (
        <Line label="Here">
          <Spans spans={t.here} />
        </Line>
      )}
    </div>
  );
}

/** Labelled lines of spans (the config teaching: What it is, Now, Allowed, Takes effect, Here). */
export function SpanLines({ lines }: { lines: [string, Span[]][] }) {
  return (
    <div className="border-l-2 border-border pl-3 text-[12.5px] leading-[19px] text-muted-foreground" data-testid="teach">
      {lines
        .filter(([, s]) => s.length > 0)
        .map(([label, s]) => (
          <Line key={label} label={label}>
            <Spans spans={s} />
          </Line>
        ))}
    </div>
  );
}

/** Teaching that folds away, so a form fits the window; open by default where there is room. */
export function Teaching({ children, label = "What Kafka says about these fields", defaultOpen = false }: { children: React.ReactNode; label?: string; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <Collapsible open={open} onOpenChange={setOpen} className="rounded-lg border bg-well/40">
      <CollapsibleTrigger className="flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-[13px] font-medium text-foreground hover:bg-well focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="teaching-toggle">
        {label}
        <ChevronDownIcon aria-hidden className={cn("size-4 text-muted-foreground transition-transform duration-150", open && "rotate-180")} />
      </CollapsibleTrigger>
      <CollapsibleContent className="flex flex-col gap-3 px-3 pt-1 pb-3">{children}</CollapsibleContent>
    </Collapsible>
  );
}

/** Kafka's refusal: its name and code, the broker's sentence, then the console's reading and what would pass. */
export function RefusalCallout({ r, testId = "refusal" }: { r: Refusal; testId?: string }) {
  return (
    <div role="alert" className="flex gap-3 rounded-xl border border-refused/30 bg-refused/5 p-4" data-testid={testId}>
      <OctagonXIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-refused" />
      <div className="min-w-0 text-[13.5px] leading-[21px]">
        <div className="flex flex-wrap items-center gap-2">
          <span className="rounded-md bg-refused/10 px-1.5 py-0.5 text-[12px] font-semibold text-refused">Refused</span>
          <code className="font-mono text-[12.5px] text-foreground" data-testid="refusal-code">
            {r.name} ({r.code})
          </code>
        </div>
        <p className="mt-1.5 font-medium break-words text-bright" data-testid="refusal-message">
          {r.message}
        </p>
        {r.rule && (
          <p className="mt-1.5 text-muted-foreground">
            <Spans spans={r.rule} />
          </p>
        )}
        {r.would_pass && (
          <p className="mt-1.5 text-muted-foreground">
            <b className="font-semibold text-foreground">What would pass:</b> <Spans spans={r.would_pass} />
          </p>
        )}
      </div>
    </div>
  );
}

/** A write the broker took. */
export function Done({ children, testId = "done" }: { children: React.ReactNode; testId?: string }) {
  return (
    <div role="status" className="flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[13.5px] leading-[21px]" data-testid={testId}>
      <CircleCheckIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-good" />
      <div className="min-w-0 break-words">{children}</div>
    </div>
  );
}

/** A field's error, under it. */
export function FieldError({ children, id }: { children?: React.ReactNode; id?: string }) {
  if (!children) return null;
  return (
    <p id={id} className="text-[12.5px] font-medium text-destructive" data-testid="field-error">
      {children}
    </p>
  );
}

/** A dialog's buttons, held at the bottom of its scrolling body so Send is always in reach. */
export function StickyFooter({ children }: { children: React.ReactNode }) {
  return <DialogFooter className="sticky -bottom-5 z-10 -mx-6 -mb-5 mt-1 border-t bg-background px-6 py-4">{children}</DialogFooter>;
}
