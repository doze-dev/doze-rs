import { getRouteApi } from "@tanstack/react-router";
import { MoreHorizontalIcon, SettingsIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CodeValue } from "@doze/ui/components/app/code-value";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Done, DoneFooter } from "@doze/ui/components/app/write-done";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@doze/ui/components/ui/dialog";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuShortcut, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@doze/ui/components/ui/popover";
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { Tooltip, TooltipContent, TooltipTrigger } from "@doze/ui/components/ui/tooltip";
import { clockUtc } from "@doze/ui/lib/format";
import { LONG, resources } from "../data";
import { Section, Specimen } from "../specimen";

const RECORDS = resources(4);
const route = getRouteApi("/overlays");

function DeleteDialog() {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState(false);
  const onOpen = (o: boolean) => {
    setOpen(o);
    if (!o) {
      setTyped("");
      setDone(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" data-testid="open-dialog">
          <Trash2Icon /> Delete queue…
        </Button>
      </DialogTrigger>
      <DialogContent data-testid="dialog">
        <DialogHeader>
          <DialogTitle>Delete orders?</DialogTitle>
          <DialogDescription>The queue and the 12 messages in it go. This cannot be undone.</DialogDescription>
        </DialogHeader>
        {done ? (
          <>
            <Done testId="dialog-done">
              <b className="font-semibold text-bright">orders</b> is deleted. Senders now get <code>QueueDoesNotExist</code>.
            </Done>
            <DoneFooter />
          </>
        ) : (
          <>
            <div className="grid gap-1.5">
              <Label htmlFor="confirm-name">Type the queue's name to confirm</Label>
              <Input id="confirm-name" value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="orders" autoComplete="off" className="font-mono" data-testid="dialog-confirm" />
            </div>
            <DialogFooter>
              <DialogClose asChild>
                <Button variant="outline">Cancel</Button>
              </DialogClose>
              <Button variant="destructive" disabled={typed !== "orders"} onClick={() => setDone(true)} data-testid="dialog-delete">
                Delete queue
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

export function OverlaysPage() {
  const { record } = route.useSearch();
  const navigate = route.useNavigate();
  const shown = RECORDS.find((r) => r.id === record);
  const [wrap, setWrap] = useState(true);
  const [order, setOrder] = useState("newest");
  const [menuSaid, setMenuSaid] = useState("nothing yet");
  const [popOpen, setPopOpen] = useState(false);
  const [limit, setLimit] = useState("100");
  return (
    <>
      <PageHeader title="Overlays" sub="Drawers and dialogs sit over the page: they never replace it. Each keeps the focus inside while open, closes on Esc, and gives the focus back to what opened it." />

      <Section id="dialog" title="Dialog" source="components/ui/dialog.tsx · components/app/write-done.tsx" note="A write made in a dialog stays open and shows what the service did, in place, with one way out: Done.">
        <Specimen label="Confirm, then done">
          <DeleteDialog />
        </Specimen>
      </Section>

      <Section id="sheet" title="Sheet" source="components/ui/sheet.tsx" note="A drawer with its own address: the row opens it by a search param, so it can be linked, and Back closes it.">
        <Specimen label="A record" stack>
          <ul className="w-full max-w-[34rem] divide-y rounded-xl border bg-card">
            {RECORDS.map((r) => (
              <li key={r.id} className="flex items-center gap-3 px-4 py-2.5">
                <code className="min-w-0 flex-1 truncate text-[13px] text-bright">{r.name}</code>
                <StatusDot tone={r.state === "Active" ? "ok" : "idle"}>{r.state}</StatusDot>
                <Button variant="outline" size="sm" onClick={() => void navigate({ search: { record: r.id } })} data-testid={`open-sheet-${r.id}`} aria-label={`Open ${r.name}`}>
                  Open
                </Button>
              </li>
            ))}
          </ul>
        </Specimen>
        <Sheet open={!!shown} onOpenChange={(o) => !o && void navigate({ search: {} })}>
          <SheetContent className="sm:max-w-md" data-testid="sheet">
            {shown && (
              <>
                <SheetHeader className="border-b p-6">
                  <SheetTitle className="font-mono text-[17px] break-all">{shown.name}</SheetTitle>
                  <SheetDescription>
                    A {shown.kind}, last called {clockUtc(shown.updated)} UTC.
                  </SheetDescription>
                </SheetHeader>
                <div className="flex flex-col gap-5 overflow-y-auto px-6">
                  <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-y-2.5 text-[13.5px]">
                    <dt className="text-muted-foreground">State</dt>
                    <dd>
                      <StatusDot tone={shown.state === "Active" ? "ok" : "idle"}>{shown.state}</StatusDot>
                    </dd>
                    <dt className="text-muted-foreground">Calls</dt>
                    <dd className="text-bright tabular-nums">{shown.calls.toLocaleString("en-US")}</dd>
                  </dl>
                  <div className="grid gap-1.5">
                    <span className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">ARN</span>
                    <CodeValue value={LONG} label="ARN" />
                  </div>
                </div>
                <SheetFooter className="mt-auto border-t p-6">
                  <Button variant="outline" onClick={() => void navigate({ search: {} })} data-testid="sheet-close">
                    Close
                  </Button>
                </SheetFooter>
              </>
            )}
          </SheetContent>
        </Sheet>
      </Section>

      <Section id="popover" title="Popover and Tooltip" source="components/ui/popover.tsx · tooltip.tsx">
        <Specimen label="Popover">
          <Popover open={popOpen} onOpenChange={setPopOpen}>
            <PopoverTrigger asChild>
              <Button variant="outline" data-testid="open-popover">
                <SettingsIcon /> Rows per page
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-64" data-testid="popover">
              <div className="grid gap-1.5">
                <Label htmlFor="pop-limit">Show at most</Label>
                <Input id="pop-limit" value={limit} onChange={(e) => setLimit(e.target.value)} inputMode="numeric" className="font-mono" data-testid="popover-input" />
              </div>
              <Button size="sm" className="mt-3" onClick={() => setPopOpen(false)} data-testid="popover-done">
                Done
              </Button>
            </PopoverContent>
          </Popover>
          <span className="font-mono text-[12.5px] text-muted-foreground" data-testid="popover-value">
            {limit}
          </span>
        </Specimen>
        <Specimen label="Tooltip">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="outline" data-testid="tooltip-trigger">
                Hover or focus me
              </Button>
            </TooltipTrigger>
            <TooltipContent>The exact value, in words</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <span tabIndex={0} className="rounded-sm text-[13.5px] underline decoration-dotted underline-offset-4 outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                3 d 4 h
              </span>
            </TooltipTrigger>
            <TooltipContent>Since 2026-11-16 05:12:40 UTC</TooltipContent>
          </Tooltip>
        </Specimen>
      </Section>

      <Section id="menu" title="DropdownMenu" source="components/ui/dropdown-menu.tsx">
        <Specimen label="Row actions">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon" aria-label="Actions for orders" data-testid="open-menu">
                <MoreHorizontalIcon />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-56" data-testid="menu">
              <DropdownMenuLabel>orders</DropdownMenuLabel>
              <DropdownMenuItem onSelect={() => setMenuSaid("Send a message")}>
                Send a message <DropdownMenuShortcut>S</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setMenuSaid("Peek")}>Peek</DropdownMenuItem>
              <DropdownMenuItem disabled>Redrive (no dead letters)</DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuCheckboxItem checked={wrap} onCheckedChange={setWrap}>
                Wrap long values
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <DropdownMenuRadioGroup value={order} onValueChange={setOrder}>
                <DropdownMenuRadioItem value="newest">Newest first</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="oldest">Oldest first</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onSelect={() => setMenuSaid("Purge")}>
                <Trash2Icon /> Purge
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <span className="text-[12.5px] text-muted-foreground" data-testid="menu-said">
            {menuSaid} · {order} · {wrap ? "wrapped" : "one line"}
          </span>
        </Specimen>
      </Section>

      <Section id="palette" title="CommandPalette" source="components/app/command-palette.tsx" note="The field in the top bar, or ⌘K / Ctrl K from anywhere. Recents first, then the app's groups; arrows move, Enter goes.">
        <Specimen label="Open it">
          <Button variant="outline" onClick={() => document.querySelector<HTMLElement>("[data-testid=palette-trigger]")?.click()} data-testid="open-palette">
            Open the palette
          </Button>
        </Specimen>
      </Section>

      <Section id="toast" title="Toast" source="components/ui/sonner.tsx" note="For something that happened elsewhere on the page (a copy, a write that left the page). Never the only place an error is said.">
        <Specimen label="Kinds">
          <Button variant="outline" onClick={() => toast.success("Message sent", { description: "orders · 1 message, 214 bytes" })} data-testid="toast-success">
            Success
          </Button>
          <Button variant="outline" onClick={() => toast.error("The queue refused it", { description: "InvalidMessageContents: the body has a character outside the allowed set." })} data-testid="toast-error">
            Error
          </Button>
          <Button variant="outline" onClick={() => toast("Purging orders", { description: LONG })}>
            Long text
          </Button>
        </Specimen>
      </Section>
    </>
  );
}
