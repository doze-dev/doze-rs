import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { CheckIcon, ChevronDownIcon } from "lucide-react";
import { useState } from "react";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@doze/ui/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@doze/ui/components/ui/popover";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../api/queries";
import { client } from "../api/client";
import { setSelectedRegion } from "../api/region";
import type { RegionRow } from "../api/types/RegionRow";
import type { RegionsView } from "../api/types/RegionsView";

/** The regions of AWS and which hold something: read from the data directory's file names, no service is called. */
export function useRegions() {
  return useQuery({ queryKey: ["regions"], queryFn: ({ signal }) => client.api<RegionsView>("/api/v1/regions", {}, signal), staleTime: 30_000 });
}

function Row({ r, current, onPick }: { r: RegionRow; current: string; onPick: (code: string) => void }) {
  return (
    <CommandItem value={`${r.code} ${r.name}`} onSelect={() => onPick(r.code)} data-testid={`region-option-${r.code}`}>
      <CheckIcon className={cn("size-4", r.code === current ? "opacity-100" : "opacity-0")} aria-hidden />
      <span className="font-mono text-[12.5px]">{r.code}</span>
      <span className="ml-auto truncate pl-3 text-xs text-muted-foreground">{r.name}</span>
    </CommandItem>
  );
}

/** The region the console looks at, in the top bar: a menu of the regions of AWS (the ones that hold something first). Choosing one makes
 * every page show that region; a page of one resource goes back to its list, since that name may not be there. Nothing is called to draw
 * the list, and a region nobody uses costs nothing. */
export function RegionMenu({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const current = useAbout().data?.region ?? "";
  const regions = useRegions().data;
  const qc = useQueryClient();
  const navigate = useNavigate();
  const path = useRouterState({ select: (s) => s.location.pathname });
  if (!current) return null;
  const pick = (code: string) => {
    setOpen(false);
    if (code === current) return;
    setSelectedRegion(code === regions?.default ? null : code);
    const detail = /^\/resources\/([^/]+)\/([^/]+)\/[^/]+/.exec(path);
    if (detail) void navigate({ to: "/resources/$service/$second", params: { service: detail[1]!, second: detail[2]! }, search: {} });
    void qc.resetQueries();
  };
  const held = (regions?.regions ?? []).filter((r) => r.has_data);
  const rest = (regions?.regions ?? []).filter((r) => !r.has_data);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button type="button" aria-label={`Region: ${current}. Choose another`} className={cn("inline-flex h-full items-center gap-1.5 rounded-l-full px-3 font-mono outline-none hover:bg-accent/40 focus-visible:ring-[3px] focus-visible:ring-ring/50", className)} data-testid="chip-region" title="The region the console looks at. Each region keeps its own resources.">
          {regions && regions.default !== current && <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-sidebar-primary" data-testid="region-not-default" title={`Not the default region (${regions.default})`} />}
          {current}
          <ChevronDownIcon className="size-3.5 text-muted-foreground" aria-hidden />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[22rem] p-0 text-foreground" data-testid="region-menu">
        <Command>
          <CommandInput placeholder="Search regions…" />
          <CommandList>
            <CommandEmpty>No region matches.</CommandEmpty>
            {held.length > 0 && (
              <CommandGroup heading="With something in it">
                {held.map((r) => <Row key={r.code} r={r} current={current} onPick={pick} />)}
              </CommandGroup>
            )}
            <CommandGroup heading={held.length > 0 ? "Every other region" : "Regions"}>
              {rest.map((r) => <Row key={r.code} r={r} current={current} onPick={pick} />)}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
