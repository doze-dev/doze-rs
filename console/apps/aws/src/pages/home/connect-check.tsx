import { Link } from "@tanstack/react-router";
import { RadioIcon, RefreshCwIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { clockMsUtc } from "@doze/ui/lib/format";
import { useHome } from "../../api/spaces";
import type { About } from "../../api/types/About";
import type { CallRecord } from "../../api/types/CallRecord";
import { clientName, who } from "../../people";
import { Card, CardSkeleton, CodeBlock } from "../card";
import { Result } from "../live/outcome";
import { ADDRESS } from "./address";
import { heardSince, probe } from "./check";

type Row = { call: CallRecord | null };

/** Check the connection: the last call that reached the address and who made it; Check again starts over from now. */
export function CheckCard({ a }: { a: About }) {
  const home = useHome();
  const [since, setSince] = useState(0);
  const h = home.data;
  const heard = h ? heardSince(h, since) : null;
  const names = new Map((h?.callers ?? []).map((c) => [c.principal.key, c]));
  const columns: DataColumn<Row>[] = [
    { id: "address", header: "Address", size: 150, min: 120, enableSorting: false, accessorFn: () => ADDRESS.label, cell: (c) => <span className="font-medium text-bright">{c.getValue<string>()}</span> },
    { id: "time", header: "Last call", size: 130, min: 120, enableSorting: false, meta: { mono: true }, accessorFn: (r) => (r.call ? clockMsUtc(r.call.at_ms) : ""), cell: (c) => c.getValue<string>() || <span className="font-sans text-muted-foreground">Waiting for a call</span> },
    {
      id: "caller",
      header: "Caller",
      size: 220,
      min: 140,
      enableSorting: false,
      accessorFn: (r) => r.call?.caller ?? "",
      cell: (c) => {
        const call = c.row.original.call;
        if (!call) return null;
        const seen = names.get(call.caller ?? "");
        const w = seen ? who(seen.principal) : { label: call.caller ?? "", title: "" };
        return (
          <span title={w.title}>
            {w.label}
            {call.sdk && <span className="ml-1.5 text-[12px] text-muted-foreground">({clientName(call.sdk)})</span>}
          </span>
        );
      },
    },
    { id: "action", header: "Operation", size: 190, min: 140, enableSorting: false, meta: { mono: true }, accessorFn: (r) => r.call?.action ?? "" },
    { id: "result", header: "Result", size: 190, min: 130, enableSorting: false, accessorFn: (r) => r.call?.outcome ?? "", cell: (c) => (c.row.original.call ? <Result call={c.row.original.call} /> : null) },
  ];
  const waiting = h !== undefined && heard === null;
  return (
    <Card
      testId="connect-check"
      title={waiting ? "Waiting for the first call" : "Check the connection"}
      lede={waiting ? "Listening on this address." : "The last call that reached this address, and who made it. This page reads it again when a call arrives."}
      actions={
        <div className="flex gap-2">
          <Button variant="outline" size="sm" className="bg-card" onClick={() => (setSince(Date.now()), void home.refetch())} disabled={home.isFetching} data-testid="check-again">
            <RefreshCwIcon /> Check again
          </Button>
          <Button asChild variant="outline" size="sm" className="bg-card">
            <Link to="/live" data-testid="check-open-live">
              <RadioIcon /> Open Live
            </Link>
          </Button>
        </div>
      }
    >
      {!h ? (
        <CardSkeleton rows={2} />
      ) : (
        <div data-testid={waiting ? "connect-waiting" : "connect-heard"} data-state={waiting ? "waiting" : "heard"}>
          <DataTable aria-label="Last call to each address" data={[{ call: heard }]} columns={columns} rowId={() => ADDRESS.id} empty="" />
          {waiting ? (
            <div className="mt-5 grid gap-3">
              <p className="text-[13.5px] text-muted-foreground">Run your app, or send one call yourself. Either ticks the row.</p>
              <CodeBlock code={probe(a.endpoint)} what="Command copied" testId="connect-probe" />
              <p className="text-[13px] text-muted-foreground">
                The check starts over from now, and only counts calls made after it.{" "}
                {since > 0 && (
                  <button type="button" className="rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setSince(0)} data-testid="check-back">
                    Back to Connect
                  </button>
                )}
              </p>
            </div>
          ) : (
            <p className="mt-4 text-[13.5px] text-muted-foreground" data-testid="connect-heard-note">
              Your app has reached this address. If an app should be calling and is not shown, its endpoint setting is not being read.
            </p>
          )}
        </div>
      )}
    </Card>
  );
}
