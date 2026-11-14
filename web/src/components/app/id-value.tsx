import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { CopyButton } from "./copy-button";

/** A long id inside a line of text or a key and value list (a member id, a producer id, a path): one line, an ellipsis when it
 * does not fit, the whole value on hover and focus, and a small Copy button. Never broken in the middle of a word. */
export function IdValue({ value, shown, what = "Copied", className }: { value: string; shown?: string; what?: string; className?: string }) {
  return (
    <span className={cn("inline-flex max-w-full min-w-0 items-center gap-1 align-middle", className)} data-slot="id-value">
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className="min-w-0 truncate rounded-sm font-mono outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
            {shown ?? value}
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-[min(90vw,40rem)] font-mono break-all">{value}</TooltipContent>
      </Tooltip>
      <CopyButton text={value} what={what} iconOnly variant="ghost" className="-my-1 size-6 shrink-0 text-muted-foreground [&_svg]:size-3.5" />
    </span>
  );
}
