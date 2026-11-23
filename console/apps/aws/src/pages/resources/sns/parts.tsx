// What the SNS pages share: the Topics | Subscriptions tabs of the service, a sentence with its names in bold, the calls that touched
// the service or one topic, and a table whose rows are chosen one at a time (the board's radio column).
import { Link } from "@tanstack/react-router";
import { LinkIcon, WaypointsIcon } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { cn } from "@doze/ui/lib/utils";
import { client, PATHS } from "../../../api/client";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { CallsPage } from "../../../api/types/CallsPage";
import type { Part } from "./sentences";

/** The two kinds of the service as the pills under the title. */
export function ServiceTabs({ active }: { active: "topics" | "subscriptions" }) {
  const pill = "inline-flex h-9 items-center gap-2 rounded-full border px-4 text-[14px] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50";
  return (
    <nav aria-label="SNS" className="mb-6 flex gap-2" data-testid="sns-tabs">
      {([["topics", "Topics", WaypointsIcon], ["subscriptions", "Subscriptions", LinkIcon]] as const).map(([id, label, Icon]) => (
        <Link key={id} to="/resources/$service/$second" params={{ service: "sns", second: id }} search={{}} aria-current={id === active ? "page" : undefined} className={cn(pill, id === active ? "border-primary/40 bg-pick text-bright" : "bg-card text-muted-foreground hover:text-foreground")} data-testid={`sns-tab-${id}`}>
          <Icon aria-hidden className="size-4" /> {label}
        </Link>
      ))}
    </nav>
  );
}

/** A sentence whose names and numbers are in bold type. */
export function Rich({ parts, className, testId }: { parts: readonly Part[]; className?: string; testId?: string }) {
  return (
    <p className={cn("max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      {parts.map((p, i) => (typeof p === "string" ? p : <b key={i} className="font-mono text-[14px] font-semibold text-bright">{p.b}</b>))}
    </p>
  );
}

/** The newest calls the record holds that SNS answered, newest first. */
export function useSnsCalls() {
  return useQuery({ queryKey: ["sns", "calls"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=200`, {}, signal), retry: false });
}

export const snsCalls = (page: CallsPage | undefined): CallRecord[] => (page?.calls ?? []).filter((c) => c.service === "sns").reverse();

/** The radio of a row: one row is chosen at a time. */
export function RadioRows({ value, onChange, children }: { value: string | null; onChange: (v: string) => void; children: React.ReactNode }) {
  return <RadioGroup value={value ?? ""} onValueChange={onChange} aria-label="Choose a row" className="block">{children}</RadioGroup>;
}

/** The radio column of a table of rows chosen one at a time. */
export function radioColumn<T>(id: (r: T) => string, noun: string): DataColumn<T> {
  return { id: "choose", size: 44, enableSorting: false, header: () => <span className="sr-only">Choose</span>, cell: (c) => <RadioGroupItem value={id(c.row.original)} aria-label={`Choose ${noun} ${id(c.row.original)}`} data-testid={`choose-${id(c.row.original)}`} /> };
}

/** A titled block of a page: the title and its sentence at the left, the block's own action at the right on the title's row. */
export function Block({ title, lede, children, testId, actions }: { title: string; lede?: React.ReactNode; children?: React.ReactNode; testId?: string; actions?: React.ReactNode }) {
  return (
    <section className="grid gap-3" aria-label={title} data-testid={testId}>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 max-w-[46rem]">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="shrink-0">{actions}</div>}
      </div>
      {children}
    </section>
  );
}
