import { CircleDashedIcon, LayersIcon, type LucideIcon, MousePointerClickIcon, PanelLeftIcon, SwatchBookIcon, Table2Icon, TextCursorInputIcon } from "lucide-react";

export type Page = { to: "/" | "/actions" | "/forms" | "/overlays" | "/data" | "/states" | "/frame"; label: string; icon: LucideIcon; keywords: string; holds: string[] };

/** The gallery's pages: one per component group. `holds` is what the palette finds each by. */
export const PAGES: Page[] = [
  { to: "/", label: "Foundations", icon: SwatchBookIcon, keywords: "roles colours type radius motion density theme", holds: ["Roles", "Type", "Radius", "Density", "Motion"] },
  { to: "/actions", label: "Actions", icon: MousePointerClickIcon, keywords: "button toggle badge copy", holds: ["Button", "Badge", "Toggle", "ToggleGroup", "CopyButton", "HeldPill"] },
  { to: "/forms", label: "Forms", icon: TextCursorInputIcon, keywords: "input select combobox field", holds: ["Input", "Textarea", "Checkbox", "RadioGroup", "Switch", "Select", "Combobox", "MultiSelect", "UtcDateTimePicker", "Form"] },
  { to: "/overlays", label: "Overlays", icon: LayersIcon, keywords: "dialog sheet drawer popover menu toast", holds: ["Dialog", "Sheet", "Popover", "Tooltip", "DropdownMenu", "CommandPalette", "Toast"] },
  { to: "/data", label: "Data", icon: Table2Icon, keywords: "table rows stat figures virtual", holds: ["StatStrip", "Figures", "DataTable", "Table", "CodeValue", "IdValue", "StatusDot", "Spans"] },
  { to: "/states", label: "States", icon: CircleDashedIcon, keywords: "loading empty error long text disabled skeleton", holds: ["Skeleton", "EmptyState", "Done"] },
  { to: "/frame", label: "Frame", icon: PanelLeftIcon, keywords: "page header breadcrumb tabs sidebar refresh card", holds: ["PageHeader", "RefreshControl", "ThemeSwitch", "TopBar", "Breadcrumb", "Tabs", "Card", "Collapsible", "ScrollArea", "Separator", "Sidebar"] },
];
