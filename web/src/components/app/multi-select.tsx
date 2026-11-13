import { AsteriskIcon, ChevronsUpDownIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import type { Option } from "./combobox";

/** What the button says of a selection: `All topics`, `orders`, `orders, payments`, `3 topics`; with the chosen shown as chips
 * under it, only how many (`1 of 30 APIs`): a name is never said twice. */
export function summary(chosen: string[], options: Option[], noun: string, chips = false): string {
  if (chosen.length === 0) return `All ${noun}`;
  if (chips) return `${chosen.length} of ${options.length} ${noun}`;
  const label = (v: string) => options.find((o) => o.value === v)?.label ?? v;
  if (chosen.length <= 2) return chosen.map(label).join(", ");
  return `${chosen.length} ${noun}`;
}

/** Several values from a list (CON-851): a button that says the selection, a popup with a search field, a checkbox per
 * option with its count, Select all and Clear; the chosen show as chips under it, each removable. */
export function MultiSelect({ options, value, onChange, noun = "items", className, triggerClassName = "w-64", "aria-label": ariaLabel, chips = true, pattern = false }: { options: Option[]; value: string[]; onChange: (v: string[]) => void; noun?: string; className?: string; triggerClassName?: string; "aria-label"?: string; chips?: boolean; /** What is typed can be added as it is (a name or a glob). */ pattern?: boolean }) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const toggle = (v: string) => onChange(value.includes(v) ? value.filter((x) => x !== v) : [...value, v]);
  // A chosen value that is no option (a pattern typed in) is listed too, so it can be unticked.
  const all: Option[] = pattern ? [...options, ...value.filter((v) => !options.some((o) => o.value === v)).map((v) => ({ value: v, label: v }))] : options;
  const ordered = [...all.filter((o) => value.includes(o.value)), ...all.filter((o) => !value.includes(o.value))];
  const own = typed.trim();
  const offer = pattern && own !== "" && !all.some((o) => o.value === own);
  return (
    <div className={cn("flex flex-col gap-2", className)} data-slot="multi-select">
      <div className="relative flex min-w-0 items-center">
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button variant="outline" role="combobox" aria-expanded={open} aria-label={ariaLabel} className={cn("min-w-0 shrink justify-between bg-card font-normal", triggerClassName)}>
              <span className="truncate" data-testid="multi-summary">
                {summary(value, options, noun, chips)}
              </span>
              <ChevronsUpDownIcon className={cn("opacity-50", value.length > 0 && "invisible")} />
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-[max(var(--radix-popover-trigger-width),16rem)] max-w-[26rem] p-0" align="start">
            <Command>
              <CommandInput placeholder={`Search ${noun}…`} value={typed} onValueChange={setTyped} />
              <CommandList>
                {!offer && <CommandEmpty>No {noun} match.</CommandEmpty>}
                {offer && (
                  <CommandGroup forceMount>
                    <CommandItem
                      forceMount
                      value={`use: ${own}`}
                      onSelect={() => {
                        onChange([...value, own]);
                        setTyped("");
                      }}
                      data-testid="multi-pattern"
                    >
                      <AsteriskIcon />
                      <span className="truncate">
                        Add <span className="font-mono text-bright">{own}</span>
                      </span>
                      <span className="ml-auto text-xs text-muted-foreground">{/[*?]/.test(own) ? "pattern" : "name"}</span>
                    </CommandItem>
                  </CommandGroup>
                )}
                <CommandGroup>
                  {ordered.map((o) => {
                    const on = value.includes(o.value);
                    return (
                      <CommandItem key={o.value} value={`${o.label} ${o.value}`} onSelect={() => toggle(o.value)} aria-selected={on} data-checked={on}>
                        <Checkbox checked={on} tabIndex={-1} aria-hidden className="pointer-events-none" />
                        <span className="truncate" title={o.label}>
                          {o.label}
                        </span>
                        {o.hint && <span className="ml-auto text-xs text-muted-foreground tabular-nums">{o.hint}</span>}
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
                <CommandSeparator />
                <CommandGroup>
                  <CommandItem onSelect={() => onChange(options.map((o) => o.value))}>Select all</CommandItem>
                  <CommandItem onSelect={() => onChange([])} disabled={value.length === 0}>
                    Clear <span className="ml-auto text-xs text-muted-foreground tabular-nums">{value.length} chosen</span>
                  </CommandItem>
                </CommandGroup>
              </CommandList>
            </Command>
          </PopoverContent>
        </Popover>
        {/* The Clear button sits inside the trigger, over its chevron, so it takes no room beside it (no gap in a toolbar) and
            the trigger never changes width: the popup is as wide as the trigger, and a trigger that shrank under an open popup
            made WebKit report a ResizeObserver loop. */}
        {value.length > 0 && (
          <Button variant="ghost" size="icon-sm" aria-label={`Clear ${noun}`} onClick={() => onChange([])} className="absolute top-1/2 right-1 size-7 -translate-y-1/2 text-muted-foreground hover:text-foreground">
            <XIcon />
          </Button>
        )}
      </div>
      {chips && value.length > 0 && (
        <div className="flex flex-wrap gap-1.5" data-testid="multi-chips">
          {value.map((v) => (
            <Badge key={v} variant="secondary" className="gap-1 pr-1 font-mono text-[11.5px]">
              <span className="max-w-[14rem] truncate">{options.find((o) => o.value === v)?.label ?? v}</span>
              <button type="button" className="rounded-full p-0.5 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" aria-label={`Remove ${v}`} onClick={() => toggle(v)}>
                <XIcon className="size-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}
    </div>
  );
}
