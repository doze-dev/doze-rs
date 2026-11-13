// Small pieces the Settings, Access and Lab pages share: the call box, Kafka's refusal, a section head, a wrapping path, a
// routed-overlay helper.
import { CodeIcon, OctagonXIcon } from "lucide-react";
import { useRef } from "react";
import type { KafkaRefusal } from "@/api/types/KafkaRefusal";
import type { Tok } from "@/api/types/Tok";
import { CopyButton } from "@/components/app/copy-button";
import { Spans } from "@/components/app/spans";
import { cn } from "@/lib/utils";

/** The Kafka call a dialog makes, line by line, as the server wrote it. */
export function CallBox({ lines, title = "The call", className }: { lines: Array<Array<[Tok, string]>>; title?: string; className?: string }) {
  return (
    <div className={cn("min-w-0 rounded-lg border bg-well p-3", className)} data-testid="call-box">
      <div className="mb-2 flex items-center gap-1.5 text-[11.5px] font-medium tracking-[0.06em] text-muted-foreground uppercase">
        <CodeIcon className="size-3.5" aria-hidden /> {title}
      </div>
      <pre className="overflow-x-auto font-mono text-[12.5px] leading-5 whitespace-pre text-foreground">
        {lines.map((l, i) => (
          <div key={i}>
            {l.length === 0
              ? " "
              : l.map(([tok, text], j) => (
                  <span key={j} className={cn(tok === "keyword" && "font-semibold text-primary", tok === "str" && "text-good")}>
                    {text}
                  </span>
                ))}
          </div>
        ))}
      </pre>
    </div>
  );
}

/** Kafka's refusal: its error name and number, its sentence, and the console's reading where it has one. */
export function RefusalPanel({ refusal, className }: { refusal: KafkaRefusal; className?: string }) {
  return (
    <div role="alert" className={cn("flex gap-3 rounded-lg border border-bad/30 bg-bad/5 p-4 text-[13.5px]", className)} data-testid="refusal">
      <OctagonXIcon className="mt-0.5 size-4 shrink-0 text-bad" aria-hidden />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2">
          <b className="font-mono font-semibold text-bad" data-testid="refusal-name">
            {refusal.name}
          </b>
          <span className="font-mono text-[12px] text-muted-foreground">({refusal.code})</span>
        </div>
        <p className="mt-1 break-words text-foreground" data-testid="refusal-message">
          {refusal.message}
        </p>
        {refusal.rule && (
          <p className="mt-2 text-muted-foreground">
            <Spans spans={refusal.rule} />
          </p>
        )}
        {refusal.would_pass && (
          <p className="mt-1 text-muted-foreground">
            <b className="font-semibold text-foreground">What would pass: </b>
            <Spans spans={refusal.would_pass} />
          </p>
        )}
      </div>
    </div>
  );
}

/** The head of a section of a long page: its name and a static sentence. */
export function SectionHead({ id, title, sub, actions }: { id: string; title: string; sub?: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
      <div className="min-w-0 flex-1 basis-[26rem]">
        <h2 id={id} className="scroll-mt-20 text-[20px] font-medium tracking-[-0.01em] text-bright">
          {title}
        </h2>
        {sub && <p className="mt-1 max-w-[52rem] text-[13.5px] leading-[21px] text-muted-foreground">{sub}</p>}
      </div>
      {actions && <div className="flex flex-none flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

/** A long value (a path) that wraps at any character instead of running out of its box, with Copy. */
export function PathValue({ value, label = "Copy path", className }: { value: string; label?: string; className?: string }) {
  return (
    <div className={cn("flex min-w-0 items-start gap-2 rounded-lg border bg-well py-1.5 pr-1.5 pl-3", className)} data-slot="path-value">
      <code className="min-w-0 flex-1 py-0.5 font-mono text-[13px] leading-5 break-all text-bright" data-testid="path-text">
        {value}
      </code>
      <CopyButton text={value} what="Path copied" label={label} size="sm" />
    </div>
  );
}

/** A key–value row of a card: the label at the left, the value and a dim line under it. */
export function Fact({ label, children, sub }: { label: string; children: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(7rem,10rem)_1fr] gap-x-4 border-b py-2.5 text-[13.5px] last:border-b-0">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">
        <div className="text-foreground">{children}</div>
        {sub && <div className="mt-0.5 text-[12.5px] text-muted-foreground">{sub}</div>}
      </dd>
    </div>
  );
}

/** An overlay with its own address: opened from the page it pushes a history entry, so closing goes Back; opened by a deep
 * link there is nothing of ours to go back to, so closing replaces the address. */
export function useRoutedOverlay(set: (patch: Record<string, string | undefined>, replace: boolean) => void) {
  const pushed = useRef(false);
  return {
    open(patch: Record<string, string | undefined>) {
      pushed.current = true;
      set(patch, false);
    },
    close(patch: Record<string, string | undefined>) {
      if (pushed.current) {
        pushed.current = false;
        window.history.back();
      } else set(patch, true);
    },
    /** Moves inside the overlay (another config, another tab) without a new history entry. */
    move(patch: Record<string, string | undefined>) {
      set(patch, true);
    },
  };
}

/** `128 MiB`, `4.0 MB`: bytes as a person reads them. */
export function bytes(n: number, si = false): string {
  const unit = si ? 1000 : 1024;
  const names = si ? ["B", "kB", "MB", "GB", "TB"] : ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = n;
  let i = 0;
  while (v >= unit && i < names.length - 1) {
    v /= unit;
    i++;
  }
  return i === 0 ? `${v} ${names[0]}` : `${v.toFixed(v < 10 ? 1 : 0)} ${names[i]}`;
}
