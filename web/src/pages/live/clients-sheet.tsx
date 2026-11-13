// The clients Sheet (board live-clients; classic pages/live/clients.rs): who is connected now, by client.id, with the software
// and the APIs each has used. Its address is `?clients=1`; a name opens the tape filtered to that client.
import { Link } from "@tanstack/react-router";
import { useClients } from "@/api/live";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { clockUtc, number, plural } from "@/lib/format";

export function ClientsSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const q = useClients(open);
  const v = q.data;
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[34rem]" data-testid="clients-sheet">
        <SheetHeader className="border-b px-6 pt-6 pb-5">
          <SheetTitle className="text-[20px] font-medium text-bright">Clients</SheetTitle>
          <SheetDescription>
            Connected now, by <span className="font-mono">client.id</span>. Software and versions come from the ApiVersions request each one sent first.
          </SheetDescription>
        </SheetHeader>
        <div className="px-6 py-4">
          {!v ? (
            <div className="flex flex-col gap-3" aria-busy>
              <Skeleton className="h-16 rounded-xl" />
              <Skeleton className="h-16 rounded-xl" />
            </div>
          ) : v.rows.length === 0 ? (
            <p className="text-[13.5px] text-muted-foreground">No client is connected. A connection that closed is gone from this list; its requests stay on the wire.</p>
          ) : (
            <>
              <p className="mb-2 text-[12.5px] text-muted-foreground" data-testid="clients-count">
                {plural(v.rows.length, "client")} · {plural(v.connections, "connection")} · as of {clockUtc(v.now_ms)} UTC
              </p>
              <ul className="flex flex-col" data-testid="clients">
                {v.rows.map((c) => (
                  <li key={c.client} className="border-b py-3.5 last:border-b-0">
                    <div className="flex min-w-0 items-center justify-between gap-3">
                      <div className="flex min-w-0 items-center gap-2">
                        <Link to="/live" search={{ client: [c.client] }} className="truncate font-mono text-[13.5px] font-medium text-bright hover:text-link hover:underline" title="Show only this client on the wire">
                          {c.client || "(no client.id)"}
                        </Link>
                        {c.tag && <span className="inline-flex h-5 items-center rounded-full border bg-well px-1.5 font-mono text-[11px] text-muted-foreground">{c.tag}</span>}
                      </div>
                      <span className="shrink-0 font-mono text-[12px] text-muted-foreground">
                        {c.peer}
                        {c.more > 0 && ` + ${c.more} more`}
                      </span>
                    </div>
                    <p className="mt-1 text-[12.5px] text-muted-foreground">
                      {c.software?.trim() && `${c.software.trim()} · `}
                      <span className="font-mono">
                        {c.apis
                          .slice(0, 6)
                          .map(([a, n]) => `${a} v${n}`)
                          .join(" · ")}
                      </span>{" "}
                      · last request at <span className="font-mono">{clockUtc(c.last_ms)}</span> UTC
                      {c.errors > 0 && ` · ${number(c.errors)} of ${number(c.requests)} not answered cleanly`}
                    </p>
                  </li>
                ))}
              </ul>
            </>
          )}
          <p className="mt-4 text-[12.5px] text-muted-foreground">Only clients that are connected now. A connection that closed is gone from this list; its requests stay on the wire.</p>
        </div>
      </SheetContent>
    </Sheet>
  );
}
