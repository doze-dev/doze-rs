import { MoonIcon } from "lucide-react";
import { cn } from "@doze/ui/lib/utils";
import type { Awake } from "./awake";

/** A service's state as a mark: a filled green dot for running, a calm blue moon for resting (it starts on its next call, and what is due
 * still runs: nothing is wrong, so it is not drawn as a warning), a faint ring for one this run does not serve. */
export function StateMark({ state, className }: { state: Awake; className?: string }) {
  if (state === "asleep") return <MoonIcon aria-hidden className={cn("size-3 shrink-0 text-link", className)} />;
  return <span aria-hidden className={cn("size-1.5 shrink-0 rounded-full", state === "awake" ? "bg-good" : "border border-muted-foreground/60", className)} />;
}
