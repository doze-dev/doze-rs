import { ClockIcon, type LucideIcon } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import { loadRecents, type Recent, RECENTS_KEY, saveRecent } from "../../lib/recents";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "../ui/command";

/** One place the palette can go to. */
export type PaletteItem = {
  label: string;
  /** The address: what the app's `onGo` navigates to, and what Recents remembers the place by. */
  to: string;
  /** The app's word for what it is (`page`, `topic`, `queue`): shown beside a recent. */
  kind: string;
  icon?: LucideIcon;
  /** A name from data (a topic, a queue, an ARN) is set in the monospace face. */
  mono?: boolean;
  /** More words it is found by. */
  keywords?: string;
  /** Something to do, not a place to go (`Dark theme`): choosing it closes the palette and calls this, `onGo` is not called
   * and Recents does not remember it. `to` is then only its key in the list. */
  run?: () => void;
};

export type PaletteGroup = {
  heading: string;
  items: PaletteItem[];
  /** A line above the group (names from the server, under the app's own pages). */
  separated?: boolean;
};

/** The ⌘K palette: Recents, then the app's groups (its pages, and the names its server found for what was typed). Every
 * group is narrowed here by the text typed, so while the server's answer for the newest text is on its way the list never
 * offers a name the next answer will take away (Enter would land on nothing). Choosing one remembers it and calls `onGo`.
 *
 * The app owns the data and the navigation: `onQueryChange` to look names up, `loading` while it does, `onGo` to go. */
export function CommandPalette({
  open,
  onOpenChange,
  returnFocus,
  groups,
  onGo,
  onQueryChange,
  loading = false,
  title = "Jump to a page",
  description = "Type a name, then Enter",
  placeholder = "Jump to…",
  recentsKey = RECENTS_KEY,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  /** The element the focus returns to when the palette closes (the top bar's trigger). */
  returnFocus: () => HTMLElement | null;
  groups: PaletteGroup[];
  onGo: (item: PaletteItem) => void;
  /** The text typed, trimmed: for an app that asks its server for names. */
  onQueryChange?: (q: string) => void;
  loading?: boolean;
  title?: string;
  description?: string;
  placeholder?: string;
  recentsKey?: string;
}) {
  const [q, setQ] = useState("");
  // Read when the palette opens (the store changes only by choosing in it).
  const recents = useMemo<Recent[]>(() => (open ? loadRecents(recentsKey) : []), [open, recentsKey]);
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
  const type = (text: string) => {
    setQ(text);
    onQueryChange?.(text.trim());
  };
  const onOpen = (o: boolean) => {
    if (!o) type("");
    onOpenChange(o);
  };
  const go = (item: PaletteItem) => {
    if (item.run) {
      onOpen(false);
      item.run();
      return;
    }
    saveRecent({ kind: item.kind, label: item.label, to: item.to }, recentsKey);
    onOpen(false);
    onGo(item);
  };
  const needle = q.trim().toLowerCase();
  const match = (label: string, keywords = "") => !needle || label.toLowerCase().includes(needle) || keywords.toLowerCase().includes(needle);
  const shownRecents = recents.filter((r) => match(r.label));
  const shown = groups.map((g) => ({ ...g, items: g.items.filter((i) => match(i.label, i.keywords)) })).filter((g) => g.items.length > 0);
  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpen}
      title={title}
      description={description}
      className="top-[18%] translate-y-0 sm:max-w-xl"
      showCloseButton={false}
      shouldFilter={false}
      onCloseAutoFocus={(e) => {
        // Safari and Firefox on macOS do not focus a button that is clicked, so "the element focused before" can be the body:
        // give the focus to the element that opened the palette (or the top bar's trigger).
        const el = returnFocus();
        if (el) {
          e.preventDefault();
          el.focus();
        }
      }}
    >
      <CommandInput placeholder={placeholder} value={q} onValueChange={type} data-testid="palette-input" />
      <CommandList className="max-h-[min(26rem,60vh)]" data-testid="palette-list">
        <CommandEmpty>{loading ? "Looking…" : `Nothing is called “${q.trim()}”.`}</CommandEmpty>
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
        {shown.map((g) => (
          <Fragment key={g.heading}>
            {g.separated && <CommandSeparator />}
            <CommandGroup heading={g.heading}>
              {g.items.map((i) => (
                <CommandItem key={`${i.kind}:${i.to}`} value={`${i.kind}:${i.to}`} onSelect={() => go(i)}>
                  {i.icon && <i.icon />}
                  <span className={i.mono ? "truncate font-mono text-[13px]" : "truncate"}>{i.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </Fragment>
        ))}
      </CommandList>
    </CommandDialog>
  );
}
