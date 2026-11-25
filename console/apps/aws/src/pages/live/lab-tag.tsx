import { FlaskConicalIcon } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@doze/ui/components/ui/tooltip";
import type { LabTag as Tag } from "../../api/types/LabTag";
import { labSentence } from "../../words";

/** The mark on a call a Lab rule touched: the rule's number, with what the rule did to the call on hover and focus. The rule's own
 * name is in the sentence, and the wire can be filtered to the calls of one rule (`?lab=3`). */
export function LabTag({ lab }: { lab: Tag }) {
  const said = labSentence(lab);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0} className="inline-flex shrink-0 items-center gap-0.5 rounded-sm border border-current px-1 py-px font-mono text-[11px] leading-none font-bold text-primary outline-none focus-visible:ring-2 focus-visible:ring-ring" data-slot="lab-tag" data-rule={lab.id} data-effect={lab.effect.kind} aria-label={said}>
          <FlaskConicalIcon aria-hidden className="size-3" />
          <span className="sr-only">Lab rule </span>{lab.id}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-[300px]">{said}</TooltipContent>
    </Tooltip>
  );
}
