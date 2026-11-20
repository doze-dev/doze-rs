import { Link } from "@tanstack/react-router";
import { cn } from "@doze/ui/lib/utils";
import type { ResourceLink } from "../../api/types/ResourceLink";
import type { ValueFormat } from "../../api/types/ValueFormat";
import { shown } from "./model";

const dim = <span className="text-muted-foreground">–</span>;

/** A link to a resource of the console. */
export function ResourceTo({ to, children, className, testId }: { to: ResourceLink; children: React.ReactNode; className?: string; testId?: string }) {
  return (
    <Link to="/resources/$service/$kind/$name" params={{ service: to.service, kind: to.kind, name: to.name }} className={cn("rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", className)} data-testid={testId}>
      {children}
    </Link>
  );
}

/** A value as its format asks, a dash when there is none; a JSON document as a block. */
export function Value({ value, format, link, block = false }: { value: string | null; format: ValueFormat; link?: ResourceLink | null; block?: boolean }) {
  if (value === null || value === "") return dim;
  if (format === "json" && block) {
    return <pre className="max-h-80 overflow-auto rounded-[10px] border bg-muted px-4 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap text-foreground">{value}</pre>;
  }
  const text = shown(value, format);
  const mono = format === "mono" || format === "json";
  const inner = (
    <span className={cn(mono && "font-mono text-[13px]")} title={text === value ? undefined : value}>
      {text}
    </span>
  );
  return link ? <ResourceTo to={link}>{inner}</ResourceTo> : inner;
}
