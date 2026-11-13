import { AsteriskIcon, CheckIcon, ChevronsUpDownIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export type Option = { value: string; label: string; hint?: string };

/** One value from a long list, with a search field (Popover + Command: shadcn's combobox). */
/** `pattern`: what is typed can be chosen as it is (a name or a glob of `*` and `?`), not only an option. */
export function Combobox({ options, value, onChange, placeholder = "Choose…", search = "Search…", empty = "Nothing matches.", className, id, "aria-label": ariaLabel, pattern = false }: { options: Option[]; value: string | null; onChange: (v: string | null) => void; placeholder?: string; search?: string; empty?: string; className?: string; id?: string; "aria-label"?: string; pattern?: boolean }) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const own = typed.trim();
  const offer = pattern && own !== "" && !options.some((o) => o.value === own);
  const chosen = options.find((o) => o.value === value) ?? (pattern && value ? { value, label: value } : undefined);
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
          <CommandInput placeholder={search} value={typed} onValueChange={setTyped} />
          <CommandList>
            {!offer && <CommandEmpty>{empty}</CommandEmpty>}
            {offer && (
              <CommandGroup forceMount>
                <CommandItem
                  forceMount
                  value={`use: ${own}`}
                  onSelect={() => {
                    onChange(own);
                    setTyped("");
                    setOpen(false);
                  }}
                  data-testid="combobox-pattern"
                >
                  <AsteriskIcon />
                  <span className="truncate">
                    Use <span className="font-mono text-bright">{own}</span>
                  </span>
                  <span className="ml-auto text-xs text-muted-foreground">{/[*?]/.test(own) ? "pattern" : "name"}</span>
                </CommandItem>
              </CommandGroup>
            )}
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
