import { CheckIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { SidebarMenuButton } from "@/components/ui/sidebar";
import { setTheme, type ThemeChoice, useTheme } from "@/lib/theme";

const CHOICES: { value: ThemeChoice; label: string; icon: typeof SunIcon }[] = [
  { value: "system", label: "System", icon: MonitorIcon },
  { value: "light", label: "Light", icon: SunIcon },
  { value: "dark", label: "Dark", icon: MoonIcon },
];

/** The theme: System (follows the OS), Light or Dark; kept in this browser. In the sidebar's footer. */
export function ThemeMenu() {
  const { choice, theme } = useTheme();
  const Icon = theme === "dark" ? MoonIcon : SunIcon;
  const label = CHOICES.find((c) => c.value === choice)!.label;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SidebarMenuButton tooltip={`Theme: ${label}`} className="text-[13px] text-muted-foreground" data-testid="theme-menu">
          <Icon />
          <span>Theme: {label}</span>
        </SidebarMenuButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-44">
        <DropdownMenuLabel className="text-[12px] text-muted-foreground">Theme</DropdownMenuLabel>
        {CHOICES.map((c) => (
          <DropdownMenuItem key={c.value} onSelect={() => setTheme(c.value)} data-testid={`theme-${c.value}`}>
            <c.icon />
            <span className="flex-1">{c.label}</span>
            {choice === c.value && <CheckIcon className="text-link" />}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
