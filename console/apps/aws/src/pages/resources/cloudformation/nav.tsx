// The navigation pane of the service's lists: Stacks and Exports (cloudformation__stacks-list, exports).
import { Link } from "@tanstack/react-router";
import { cn } from "@doze/ui/lib/utils";

/** `Navigation pane  Stacks  Exports`: the two kinds of the service, the one on screen in the accent. */
export function Nav({ active }: { active: "stacks" | "exports" }) {
  return (
    <nav aria-label="CloudFormation" className="mb-5 flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[14px]" data-testid="cfn-nav">
      <span className="text-muted-foreground">Navigation pane</span>
      {(["stacks", "exports"] as const).map((id) => (
        <Link key={id} to="/resources/$service/$second" params={{ service: "cloudformation", second: id }} search={{}} aria-current={id === active ? "page" : undefined} className={cn("rounded-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", id === active ? "font-semibold text-link" : "text-link hover:underline")} data-testid={`cfn-nav-${id}`}>
          {id === "stacks" ? "Stacks" : "Exports"}
        </Link>
      ))}
    </nav>
  );
}
