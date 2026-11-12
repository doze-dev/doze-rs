import { CheckIcon, ChevronsUpDownIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export type Option = { value: string; label: string; hint?: string };

/** One value from a long list, with a search field (Popover + Command: shadcn's combobox). */
export function Combobox({ options, value, onChange, placeholder = "Choose…", search = "Search…", empty = "Nothing matches.", className, id, "aria-label": ariaLabel }: { options: Option[]; value: string | null; onChange: (v: string | null) => void; placeholder?: string; search?: string; empty?: string; className?: string; id?: string; "aria-label"?: string }) {
  const [open, setOpen] = useState(false);
  const chosen = options.find((o) => o.value === value);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button id={id} variant="outline" role="combobox" aria-expanded={open} aria-label={ariaLabel} className={cn("w-64 justify-between bg-card font-normal", !chosen && "text-muted-foreground", className)}>
          <span className="truncate">{chosen ? chosen.label : placeholder}</span>
          <ChevronsUpDownIcon className="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[max(var(--radix-popover-trigger-width),16rem)] max-w-[26rem] p-0" align="start">
        <Command>
          <CommandInput placeholder={search} />
          <CommandList>
            <CommandEmpty>{empty}</CommandEmpty>
            <CommandGroup>
              {options.map((o) => (
                <CommandItem
                  key={o.value}
                  value={`${o.label} ${o.value}`}
                  onSelect={() => {
                    onChange(o.value === value ? null : o.value);
                    setOpen(false);
                  }}
                >
                  <CheckIcon className={cn(o.value === value ? "opacity-100" : "opacity-0")} />
                  <span className="truncate" title={o.label}>
                    {o.label}
                  </span>
                  {o.hint && <span className="ml-auto text-xs text-muted-foreground tabular-nums">{o.hint}</span>}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
