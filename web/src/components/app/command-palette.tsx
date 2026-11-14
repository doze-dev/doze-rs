import { useNavigate } from "@tanstack/react-router";
import { ClockIcon, Rows3Icon, UsersIcon } from "lucide-react";
import { useDeferredValue, useEffect, useMemo, useState } from "react";
import { useJump } from "@/api/queries";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { loadRecents, type Recent, saveRecent } from "@/lib/recents";
import { PAGES } from "./nav";

const segment = (s: string) => encodeURIComponent(s);

/** The ⌘K palette: Recents, Pages, Topics, Groups. Names come from `/api/v1/jump?q=` (the server filters); pages and recents
 * are filtered here. Choosing one goes there and remembers it. */
export function CommandPalette({ open, onOpenChange, returnFocus }: { open: boolean; onOpenChange: (o: boolean) => void; returnFocus: () => HTMLElement | null }) {
  const [q, setQ] = useState("");
  const deferred = useDeferredValue(q.trim());
  const jump = useJump(deferred, open);
  const navigate = useNavigate();
  // Read when the palette opens (the store changes only by choosing in it).
  const recents = useMemo<Recent[]>(() => (open ? loadRecents() : []), [open]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        onOpenChange(!open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onOpenChange]);
  const onOpen = (o: boolean) => {
    if (!o) setQ("");
    onOpenChange(o);
  };
  const go = (r: Recent) => {
    saveRecent(r);
    onOpen(false);
    void navigate({ to: r.to });
  };
  const needle = q.trim().toLowerCase();
  const pages = PAGES.filter((p) => !needle || p.label.toLowerCase().includes(needle) || (p.keywords ?? "").includes(needle));
  const shownRecents = recents.filter((r) => !needle || r.label.toLowerCase().includes(needle));
  // While the answer for the newest text is on its way the previous one is shown (keepPreviousData), narrowed here by the
  // same rule, so the list never offers a name the next answer will take away (Enter would land on nothing).
  const match = (n: string) => !needle || n.toLowerCase().includes(needle);
  const topics = (jump.data?.topics ?? []).filter(match);
  const groups = (jump.data?.groups ?? []).filter(match);
  return (
    <CommandDialog open={open} onOpenChange={onOpen} title="Jump to a topic or group" description="Type a name, then Enter" className="top-[18%] translate-y-0 sm:max-w-xl" showCloseButton={false} shouldFilter={false} onCloseAutoFocus={(e) => {
        // Safari and Firefox on macOS do not focus a button that is clicked, so "the element focused before" can be the body:
        // give the focus to the element that opened the palette (or the top bar's trigger).
        const el = returnFocus();
        if (el) {
          e.preventDefault();
          el.focus();
        }
      }}>
      <CommandInput placeholder="Jump to a topic, a group or a page…" value={q} onValueChange={setQ} data-testid="palette-input" />
      <CommandList className="max-h-[min(26rem,60vh)]" data-testid="palette-list">
        <CommandEmpty>{jump.isFetching ? "Looking…" : `Nothing is called “${q.trim()}”.`}</CommandEmpty>
        {shownRecents.length > 0 && (
          <CommandGroup heading="Recents">
            {shownRecents.map((r) => (
              <CommandItem key={`recent:${r.to}`} value={`recent:${r.to}`} onSelect={() => go(r)}>
                <ClockIcon />
                <span className="truncate">{r.label}</span>
                <span className="ml-auto text-xs text-muted-foreground">{r.kind}</span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {pages.length > 0 && (
          <CommandGroup heading="Pages">
            {pages.map((p) => (
              <CommandItem key={`page:${p.to}`} value={`page:${p.to}`} onSelect={() => go({ kind: "page", label: p.label, to: p.to })}>
                <p.icon />
                {p.label}
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {topics.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Topics">
              {topics.map((t) => (
                <CommandItem key={`topic:${t}`} value={`topic:${t}`} onSelect={() => go({ kind: "topic", label: t, to: `/topics/${segment(t)}` })}>
                  <Rows3Icon />
                  <span className="truncate font-mono text-[13px]">{t}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
        {groups.length > 0 && (
          <CommandGroup heading="Groups">
            {groups.map((g) => (
              <CommandItem key={`group:${g}`} value={`group:${g}`} onSelect={() => go({ kind: "group", label: g, to: `/groups/${segment(g)}` })}>
                <UsersIcon />
                <span className="truncate font-mono text-[13px]">{g}</span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}
