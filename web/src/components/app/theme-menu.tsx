import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { setTheme, type ThemeChoice, useTheme } from "@/lib/theme";

const CHOICES: { value: ThemeChoice; label: string; icon: typeof SunIcon }[] = [
  { value: "system", label: "System", icon: MonitorIcon },
  { value: "light", label: "Light", icon: SunIcon },
  { value: "dark", label: "Dark", icon: MoonIcon },
];

/** The theme as three icons, System, Light and Dark, the chosen one filled in yellow; kept in this browser. In the top bar.
 * A radio group: one is always chosen, arrow keys move between them. */
export function ThemeSwitch() {
  const { choice } = useTheme();
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      value={choice}
      onValueChange={(v) => v && setTheme(v as ThemeChoice)}
      aria-label="Theme"
      className="inline-flex h-[30px] shrink-0 items-center gap-0.5 rounded-[9px] border bg-card p-0.5"
      data-testid="theme-switch"
    >
      {CHOICES.map((c) => (
        <Tooltip key={c.value}>
          <TooltipTrigger asChild>
            <ToggleGroupPrimitive.Item
              value={c.value}
              aria-label={c.label}
              data-testid={`theme-${c.value}`}
              className="inline-flex size-6 items-center justify-center rounded-[7px] text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none data-[state=on]:bg-primary data-[state=on]:text-primary-foreground [&_svg]:size-3.5"
            >
              <c.icon aria-hidden />
            </ToggleGroupPrimitive.Item>
          </TooltipTrigger>
          <TooltipContent>{c.label}</TooltipContent>
        </Tooltip>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}
