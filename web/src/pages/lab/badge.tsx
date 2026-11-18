// The Lab's tag on a request, for the Live tape and its drawer (slice R3 mounts it): a small `lab` badge whose tooltip says
// which rule touched the request and what it did, and links to the rule. Kept out of the Lab page's chunk so Live can import it
// without pulling the Lab in.
import { Link } from "@tanstack/react-router";
import { FlaskConicalIcon } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** The words under the badge, for a drawer that has room for a sentence. */
export function labTagSentence(name: string, did: string): string {
  return `The Lab rule "${name}" ${did}. The rule, not the broker, did this: the broker's data is untouched, and when the rule ends requests behave as before.`;
}

/** `lab`: a request a Lab rule touched. `rule` is its number (the link to `/lab?rule=`), `did` what it did, in words
 * (`made the answer wait 400 ms more`, `answered REQUEST_TIMED_OUT without asking the broker`). */
export function LabRuleBadge({ rule, name, did, className }: { rule: number; name: string; did: string; className?: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link
          to="/lab"
          search={{ rule }}
          className={cn("inline-flex h-5 items-center gap-1 rounded-full border border-warn/40 bg-warn/10 px-2 font-mono text-[11px] font-medium text-warn hover:bg-warn/20 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", className)}
          aria-label={`Lab: ${name}, ${did}`}
          data-testid="lab-badge"
          data-rule={rule}
        >
          <FlaskConicalIcon className="size-3" aria-hidden /> lab
        </Link>
      </TooltipTrigger>
      <TooltipContent className="max-w-80">
        <b className="font-semibold">{name}</b> (rule {rule}) {did}. Open the rule to see every request it touched.
      </TooltipContent>
    </Tooltip>
  );
}
