// A table of subscriptions as the boards draw it: an id, the endpoint (its name, and where it is), a status and the protocol. Used by the
// topic's Subscriptions tab and by the Subscriptions list (which adds the topic).
import { Link } from "@tanstack/react-router";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { cn } from "@doze/ui/lib/utils";
import { radioColumn } from "./parts";
import { endpointName, pendingWhy, type SubRow, shortArn, shortId, statusOf } from "./sub-model";
import { protocolLabel, storedWhy } from "./protocols";

/** The status with its dot: green for confirmed, amber for one waiting. */
export function Status({ s }: { s: Pick<SubRow, "pending"> & { protocol?: string } }) {
  const why = s.protocol ? storedWhy(s.protocol) : null;
  return (
    <span className="grid gap-0.5" data-testid="sub-status-cell">
      <span className={cn("inline-flex items-center gap-2 text-[13.5px] font-medium", s.pending ? "text-warn" : "text-good")} data-testid="sub-status">
        <span aria-hidden className="size-2 rounded-full bg-current" />
        {statusOf(s)}
      </span>
      {why && (
        <span className="text-[12px] leading-4 text-muted-foreground" data-testid="not-active">
          <b className="font-semibold text-foreground">Not active locally.</b> {why}
        </span>
      )}
    </span>
  );
}

/** The endpoint: its name in bold and, under it, the ARN with the account left out; a URL or an address as it is, an address with why it waits. */
export function EndpointCell({ s }: { s: SubRow }) {
  const arn = s.protocol === "sqs" || s.protocol === "lambda";
  const mail = s.protocol === "email" || s.protocol === "email-json";
  return (
    <div className="grid leading-5">
      <span className="truncate text-[13.5px] font-medium text-bright" title={s.endpoint}>{endpointName(s.protocol, s.endpoint)}</span>
      {arn ? <span className="truncate font-mono text-[11.5px] text-muted-foreground" title={s.endpoint}>{shortArn(s.endpoint)}</span> : mail && s.pending ? <span className="font-mono text-[11.5px] text-muted-foreground">{pendingWhy(s.protocol)}</span> : null}
    </div>
  );
}

/** The columns of the topic's tab (`radio`) and of the list (`topic`: the topic's ARN as the board draws it, `arn:…:orders-created`). */
export function subColumns({ radio, topic }: { radio: boolean; topic: boolean }): DataColumn<SubRow>[] {
  return [
    ...(radio ? [radioColumn<SubRow>((r) => r.name, "subscription")] : []),
    { id: "id", header: "Subscription ID", size: 190, accessorFn: (r) => r.id ?? "", meta: { mono: true }, cell: (c) => {
      const s = c.row.original;
      return <Link to="/resources/$service/$kind/$name" params={{ service: "sns", kind: "subscriptions", name: s.name }} className="font-mono text-[13px] text-link hover:underline" title={s.id ?? "Waiting for its endpoint to confirm"} data-testid="row-link">{shortId(s.id)}</Link>;
    } },
    { id: "endpoint", header: "Endpoint", size: 360, min: 220, accessorFn: (r) => r.endpoint, cell: (c) => <EndpointCell s={c.row.original} /> },
    { id: "status", header: "Status", size: 210, accessorFn: (r) => (r.pending ? 1 : 0), cell: (c) => <Status s={c.row.original} /> },
    { id: "protocol", header: "Protocol", size: 220, accessorFn: (r) => protocolLabel(r.protocol), cell: (c) => <span className="text-[13.5px]">{protocolLabel(c.row.original.protocol)}</span> },
    ...(topic ? [{ id: "topic", header: "Topic ARN", size: 240, min: 200, accessorFn: (r: SubRow) => r.topicArn, meta: { mono: true }, cell: (c: { row: { original: SubRow } }) => <span className="font-mono text-[13px]" title={c.row.original.topicArn}>arn:…:{c.row.original.topic}</span> }] : []),
  ];
}
