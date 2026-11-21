// The streams of a log group, newest event first, each a link to the stream viewer: Lambda's old Invocations tab drew them, and the
// Logs pages still do (`../logs/streams-tab.tsx`).
import { Link } from "@tanstack/react-router";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { at } from "../deep/parts";

export type LogStream = { logStreamName: string; lastEventTimestamp?: number; firstEventTimestamp?: number; creationTime?: number; storedBytes?: number };

/** The streams of a log group, newest event first, each a link to the stream viewer. */
export function StreamTable({ group, streams }: { group: string; streams: LogStream[] }) {
  const columns: DataColumn<LogStream>[] = [
    { id: "s", header: "Log stream", accessorKey: "logStreamName", meta: { mono: true }, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "logs", kind: "streams", name: c.row.original.logStreamName }} search={{ group }} className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="stream-link">{c.row.original.logStreamName}</Link> },
    { id: "l", header: "Last event", size: 230, accessorFn: (r) => r.lastEventTimestamp ?? 0, cell: (c) => at(c.row.original.lastEventTimestamp) },
    { id: "f", header: "First event", size: 230, accessorFn: (r) => r.firstEventTimestamp ?? 0, cell: (c) => at(c.row.original.firstEventTimestamp) },
  ];
  return <DataTable aria-label="Log streams" data={streams} columns={columns} rowId={(r) => r.logStreamName} maxHeight={420} empty="No streams yet." />;
}
