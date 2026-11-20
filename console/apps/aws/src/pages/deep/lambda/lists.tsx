// A function's versions and aliases (ListVersionsByFunction, ListAliases), and its recent invocations: the Invoke calls the
// Live record holds for it, and the newest streams of its log group, each opening in the stream viewer.
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { clockUtc } from "@doze/ui/lib/format";
import { errorWords } from "@doze/ui/components/app/alert";
import { useDeepRead } from "../../../api/deep";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Result } from "../../live/outcome";
import { isConsole } from "../../live/search";
import { Loaded, ReadWith, Section, when } from "../parts";

type Version = { Version: string; Description?: string; LastModified?: string; FunctionArn?: string; CodeSha256?: string };
type Alias = { Name: string; Description?: string; FunctionVersion?: string; AliasArn?: string; RoutingConfig?: { AdditionalVersionWeights?: Record<string, number> } };
export type LogStream = { logStreamName: string; lastEventTimestamp?: number; firstEventTimestamp?: number; creationTime?: number; storedBytes?: number };

const mono = (s: string | undefined) => <span className="font-mono text-[13px]">{s || "–"}</span>;

const VERSION_COLUMNS: DataColumn<Version>[] = [
  { id: "v", header: "Version", size: 110, accessorKey: "Version", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.Version}{c.row.original.Version === "$LATEST" && <span className="ml-2 font-sans font-normal text-muted-foreground">unpublished</span>}</span> },
  { id: "d", header: "Description", accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description || <span className="text-muted-foreground">–</span> },
  { id: "m", header: "Last modified", size: 210, accessorFn: (r) => r.LastModified ?? "", cell: (c) => when(c.row.original.LastModified) },
  { id: "a", header: "Function ARN", size: 420, accessorFn: (r) => r.FunctionArn ?? "", meta: { mono: true }, cell: (c) => mono(c.row.original.FunctionArn) },
];

const ALIAS_COLUMNS: DataColumn<Alias>[] = [
  { id: "n", header: "Name", size: 160, accessorKey: "Name", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.Name}</span> },
  { id: "v", header: "Version", size: 100, accessorFn: (r) => r.FunctionVersion ?? "", meta: { mono: true }, cell: (c) => mono(c.row.original.FunctionVersion) },
  { id: "d", header: "Description", accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description || <span className="text-muted-foreground">–</span> },
  { id: "a", header: "Alias ARN", size: 420, accessorFn: (r) => r.AliasArn ?? "", meta: { mono: true }, cell: (c) => mono(c.row.original.AliasArn) },
];

export function FunctionVersions({ name }: { name: string }) {
  const versions = useDeepRead<{ Versions?: Version[] }>("lambda", "ListVersionsByFunction", { FunctionName: name, MaxItems: 50 });
  const aliases = useDeepRead<{ Aliases?: Alias[] }>("lambda", "ListAliases", { FunctionName: name, MaxItems: 50 });
  return (
    <div className="grid gap-6">
      <Section title="Versions" lede="A version is a copy of the code and settings that does not change. $LATEST is the one being edited; numbers only go up." testId="lambda-versions">
        <Loaded q={versions} what="The versions">
          {(v) => <DataTable aria-label="Versions" data={v.Versions ?? []} columns={VERSION_COLUMNS} rowId={(r) => r.Version} maxHeight={420} empty="No versions." />}
        </Loaded>
        <ReadWith ops={["ListVersionsByFunction"]} more="Publishing a version is done with aws lambda publish-version." />
      </Section>
      <Section title="Aliases" lede="An alias names a version; callers use the alias and are moved when it is." testId="lambda-aliases">
        <Loaded q={aliases} what="The aliases">
          {(a) => (a.Aliases ?? []).length === 0 ? <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="aliases-none">None. Make one with aws lambda create-alias.</p> : <DataTable aria-label="Aliases" data={a.Aliases ?? []} columns={ALIAS_COLUMNS} rowId={(r) => r.Name} maxHeight={420} />}
        </Loaded>
        <ReadWith ops={["ListAliases"]} />
      </Section>
    </div>
  );
}

const CALL_COLUMNS: DataColumn<CallRecord>[] = [
  { id: "time", header: "Time", size: 120, accessorKey: "at_ms", meta: { mono: true }, cell: (c) => clockUtc(c.row.original.at_ms) },
  { id: "caller", header: "Caller", size: 240, accessorFn: (r) => r.caller ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.caller ?? "–"}{c.row.original.via?.by ? <span className="text-muted-foreground"> via {c.row.original.via.by}</span> : null}</span> },
  { id: "result", header: "Result", accessorFn: (r) => r.status, cell: (c) => <Result call={c.row.original} /> },
];

/** The streams of a log group, newest event first, each a link to the stream viewer. */
export function StreamTable({ group, streams }: { group: string; streams: LogStream[] }) {
  const columns: DataColumn<LogStream>[] = [
    { id: "s", header: "Log stream", accessorKey: "logStreamName", meta: { mono: true }, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "logs", kind: "streams", name: c.row.original.logStreamName }} search={{ group }} className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="stream-link">{c.row.original.logStreamName}</Link> },
    { id: "l", header: "Last event", size: 230, accessorFn: (r) => r.lastEventTimestamp ?? 0, cell: (c) => when(c.row.original.lastEventTimestamp) },
    { id: "f", header: "First event", size: 230, accessorFn: (r) => r.firstEventTimestamp ?? 0, cell: (c) => when(c.row.original.firstEventTimestamp) },
  ];
  return <DataTable aria-label="Log streams" data={streams} columns={columns} rowId={(r) => r.logStreamName} maxHeight={420} empty="No streams yet." />;
}

export function FunctionInvocations({ d }: { d: ResourceDetail }) {
  const [mine, setMine] = useState(false);
  const group = `/aws/lambda/${d.name}`;
  const streams = useDeepRead<{ logStreams?: LogStream[] }>("logs", "DescribeLogStreams", { logGroupName: group, orderBy: "LastEventTime", descending: true, limit: 10 });
  const invokes = d.calls.filter((c) => c.action === "Invoke" || c.action === "InvokeWithResponseStream");
  const shown = mine ? invokes : invokes.filter((c) => !isConsole(c));
  const hidden = invokes.length - invokes.filter((c) => !isConsole(c)).length;
  const missing = streams.error && errorWords(streams.error).code === "ResourceNotFoundException";
  return (
    <div className="grid gap-6">
      <Section
        title="Recent invocations"
        lede={
          <>
            The Invoke calls in the Live record that ran {d.name}, newest first; read when this page was.{" "}
            {(hidden > 0 || mine) && (
              <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setMine(!mine)} data-testid="invokes-console-toggle">
                {mine ? "Hide this console's own" : `Show ${hidden} made by this console`}
              </button>
            )}
          </>
        }
        testId="lambda-invocations"
      >
        <DataTable aria-label="Recent invocations" data={shown} columns={CALL_COLUMNS} rowId={(r) => String(r.seq)} maxHeight={420} empty={hidden > 0 && !mine ? "Only this console has invoked it so far." : "No invocation in the Live record yet."} />
      </Section>
      <Section title="Log streams" lede={<>Each run writes to a stream of <span className="font-mono">{group}</span>; the newest ten. Open one to read it.</>} testId="lambda-streams">
        {missing ? (
          <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="streams-none">No log group yet: Lambda makes it when the function first runs.</p>
        ) : (
          <Loaded q={streams} what="The log streams">
            {(s) => <StreamTable group={group} streams={s.logStreams ?? []} />}
          </Loaded>
        )}
        <ReadWith ops={["DescribeLogStreams"]} />
      </Section>
    </div>
  );
}
