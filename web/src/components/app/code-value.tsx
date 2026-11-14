import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { CopyButton } from "./copy-button";

/** A long value (a path, an id, an address) on one line: an ellipsis when it does not fit, the whole value on hover and focus,
 * and a Copy button. */
export function CodeValue({ value, className, copy = true, label }: { value: string; className?: string; copy?: boolean; label?: string }) {
  return (
    <div className={cn("flex min-w-0 items-center gap-2 rounded-lg border bg-well py-1 pr-1 pl-3", className)} data-slot="code-value">
      <Tooltip>
        <TooltipTrigger asChild>
          <code tabIndex={0} className="min-w-0 flex-1 truncate font-mono text-[13px] text-bright outline-none focus-visible:ring-2 focus-visible:ring-ring/50 rounded-sm">
            {value}
          </code>
        </TooltipTrigger>
        <TooltipContent className="max-w-[min(90vw,40rem)] font-mono break-all">{value}</TooltipContent>
      </Tooltip>
      {copy && <CopyButton text={value} what={label ? `${label} copied` : "Copied"} iconOnly variant="ghost" label={label ?? "Copy"} />}
    </div>
  );
}
