import { cn } from "@doze/ui/lib/utils";
import mark from "../assets/doze-aws.svg";

/** The doze-aws mark as the brand draws it (doze-website/public/brand/doze-aws.svg, copied unchanged): never redrawn here. */
export function Logo({ size, className }: { size: number; className?: string }) {
  return <img src={mark} alt="" width={size} height={size} draggable={false} className={cn("shrink-0 select-none", className)} style={{ width: size, height: size }} />;
}

/** The name as the boards write it: `doze-aws`, in the text face, semibold. */
export function Wordmark({ className }: { className?: string }) {
  return <span className={cn("font-display font-semibold tracking-[-0.01em] whitespace-nowrap text-bright", className)}>doze-aws</span>;
}
