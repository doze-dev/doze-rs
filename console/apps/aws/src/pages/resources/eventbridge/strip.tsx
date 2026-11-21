// The section strip of EventBridge's lists (`Event buses  Rules  Archives  Replays  API destinations`): the same five on every list, the one
// on screen marked.
import { Link } from "@tanstack/react-router";
import { cn } from "@doze/ui/lib/utils";

export const SECTIONS = [
  { kind: "buses", label: "Event buses" },
  { kind: "rules", label: "Rules" },
  { kind: "archives", label: "Archives" },
  { kind: "replays", label: "Replays" },
  { kind: "api-destinations", label: "API destinations" },
] as const;

export function SectionStrip({ current }: { current: (typeof SECTIONS)[number]["kind"] }) {
  return (
    <nav aria-label="EventBridge sections" className="mb-6 flex flex-wrap gap-1 border-b" data-testid="section-strip">
      {SECTIONS.map((s) => (
        <Link key={s.kind} to="/resources/$service/$second" params={{ service: "eventbridge", second: s.kind }} search={{}} aria-current={s.kind === current ? "page" : undefined} className={cn("-mb-px border-b-2 px-3 py-2.5 text-[14px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50", s.kind === current ? "border-primary font-medium text-bright" : "border-transparent text-muted-foreground hover:text-bright")} data-testid={`strip-${s.kind}`}>
          {s.label}
        </Link>
      ))}
    </nav>
  );
}
