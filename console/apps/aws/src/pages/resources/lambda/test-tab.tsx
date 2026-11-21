// A function's Test tab (lambda__function-test, test-result-ok, test-result-error, test-result-throttled): an event typed as JSON
// or started from a template shaped like the function's own triggers, invoked synchronously as the console (Live shows the
// Invoke), and what came back: the answer or the error the function threw, the tail of its log, and the fields of its REPORT
// line; or Lambda's refusal (throttled), with its details.
import { PlayIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import type { Invoked } from "../../../api/types/Invoked";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { CallPanel, Field } from "../../../kit";
import { on, useOp } from "../deep/api";
import { Card, CodeBlock, pretty, Refused } from "../deep/parts";
import { notFound } from "./hooks";
import type { Fn, Trigger } from "./model";
import { reportOf } from "./report";
import { firstEvent, templatesFor } from "./templates";

/** The most a page sends: the console takes a request body of at most 1 MiB (Lambda itself takes 6 MB). */
const MOST = 1 << 20;

/** What an Invoke that failed says: its code and words, and the status the console passed on. */
function Throttled({ error, fn }: { error: ApiFailure; fn: Fn }) {
  const zero = fn.reserved === 0;
  return (
    <div className="mt-8 grid gap-6" data-testid="invoke-refused">
      <div>
        <h3 className="text-[15px] font-semibold text-bright">Execution result</h3>
        <p className="mt-2 text-[14px] text-foreground" data-testid="invoke-refused-words">
          Invoke returned <span className="font-mono">{error.code}</span>.{error.code === "TooManyRequestsException" ? " The request throughput limit was exceeded. The function did not run." : ` ${error.message}`}
        </p>
      </div>
      <div>
        <h3 className="text-[15px] font-semibold text-bright">Error details</h3>
        <dl className="mt-3 grid max-w-[640px] grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="invoke-refused-details">
          {[["Error", error.code], ["HTTP status", String(error.status)], ["Message", error.message]].map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="border-b py-2.5 text-[13.5px] text-muted-foreground">{k}</dt>
              <dd className="border-b py-2.5 font-mono text-[13px] break-words text-bright">{v}</dd>
            </div>
          ))}
        </dl>
        {error.code === "TooManyRequestsException" && (
          <p className="mt-3 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="invoke-refused-teach">
            A throttled call never reaches your code. Throttled requests do not count as Invocations or Errors.{" "}
            <span className="text-muted-foreground">{zero ? "Setting a function's reserved concurrency to 0 throttles it on purpose; here that is why the call was refused." : `${fn.name} has ${fn.reserved === null ? "no reserved concurrency set, so the limit that was reached is the account's." : `a reserved concurrency of ${fn.reserved}, which every run in progress has used.`}`}</span>
          </p>
        )}
      </div>
    </div>
  );
}

function Result({ r }: { r: Invoked }) {
  const report = r.log ? reportOf(r.log) : [];
  const threw = r.function_error !== null;
  const rows = threw ? [report[0], ["FunctionError", r.function_error ?? ""] as [string, string], ...report.slice(1)].filter(Boolean) as [string, string][] : report;
  return (
    <div className="mt-8 grid gap-6" data-testid="invoke-result">
      <div>
        <h3 className="text-[15px] font-semibold text-bright">Execution result</h3>
        {threw ? (
          <p className="mt-2 text-[14px] text-foreground" data-testid="invoke-status">The function returned an error. The response is an error object instead of a result.</p>
        ) : (
          <div className="mt-2 flex flex-wrap items-center gap-3 text-[13.5px]">
            <StatusDot tone="ok" data-testid="invoke-status">Status: Succeeded</StatusDot>
            {r.executed_version && <span className="text-muted-foreground">ran <span className="font-mono">{r.executed_version}</span></span>}
          </div>
        )}
      </div>
      <div>
        <h3 className="mb-2 text-[15px] font-semibold text-bright">Response</h3>
        {r.payload ? <CodeBlock text={pretty(r.payload)} testId="invoke-payload" max="max-h-80" /> : <p className="text-[13.5px] text-muted-foreground">The function answered with nothing.</p>}
        {threw && (
          <p className="mt-3 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="invoke-error-teach">
            The call itself succeeded: the HTTP status is still {r.status}, and the <span className="font-mono">FunctionError</span> header (<span className="font-mono">X-Amz-Function-Error</span>) says {r.function_error}.
            {r.payload.includes("/code/") && <span className="text-muted-foreground"> The file path in the trace is where doze-aws unpacked the code on this machine.</span>}
          </p>
        )}
      </div>
      {r.log !== null && (
        <div>
          <h3 className="mb-2 text-[15px] font-semibold text-bright">Function Logs</h3>
          <CodeBlock text={r.log} testId="invoke-log" max="max-h-80" />
          <p className="mt-2 max-w-[46rem] text-[12.5px] text-muted-foreground">The last 4 KB of the log come back with the response. The REPORT line is the same line CloudWatch Logs keeps. Init Duration appears only on a cold start. Memory Size is the configured value; the function has no memory limit locally.</p>
        </div>
      )}
      {rows.length > 0 && (
        <div>
          <h3 className="text-[15px] font-semibold text-bright">Details</h3>
          <p className="mt-1 text-[13px] text-muted-foreground">The Request ID and the fields of the REPORT line.</p>
          <dl className="mt-3 grid max-w-[640px] grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="invoke-report">
            {rows.map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="border-b py-2.5 text-[13.5px] text-muted-foreground">{k}</dt>
                <dd className="border-b py-2.5 font-mono text-[13px] text-bright">{v}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}
    </div>
  );
}

const KIND_WORDS: Record<string, string> = { "SNS topic": "topic", "SQS queue": "queue", "S3 bucket": "bucket", "EventBridge rule": "rule" };

export function TestTab({ d, f, triggers, where }: { d: ResourceDetail; f: Fn; triggers: readonly Trigger[]; where: { region: string; account: string } }) {
  const templates = useMemo(() => templatesFor(triggers, where), [triggers, where]);
  const start = useMemo(() => firstEvent(templates), [templates]);
  const [picked, setPicked] = useState<string | null>(null);
  const [typed, setTyped] = useState<string | null>(null);
  const [tail] = useState(true);
  const chosen = picked ?? start.id;
  const event = typed ?? start.text;
  const invoke = useOp<Invoked, { Payload: string; LogType: "Tail" | "None" }>(on.fn(d.name), "Invoke", { keep: true });
  const check = checkJson(event);
  const big = new TextEncoder().encode(event).length > MOST;
  const own = templates.find((t) => t.id === chosen);
  const run = (e: React.FormEvent) => {
    e.preventDefault();
    if (!check.ok || big || invoke.isPending) return;
    invoke.mutate({ Payload: event.trim() === "" ? "" : event, LogType: tail ? "Tail" : "None" });
  };
  const refused = invoke.error instanceof ApiFailure && !notFound(invoke.error) ? invoke.error : null;
  const shape = own?.of ? `The event below has the shape the ${own.label.replace(/^\S+ \S+ /, "")} ${KIND_WORDS[own.of] ?? "source"} sends to ${d.name}.` : null;
  return (
    <Card title="Test event" lede="Lambda invokes the function synchronously with this event and shows the result below." testId="lambda-test">
      <form onSubmit={run} className="grid gap-5">
        <Field label={<>Template <span className="font-normal text-muted-foreground">Optional</span></>} help={<>An example event to start from.{shape ? ` ${shape}` : ""}</>} testId="field-template">
          {(id) => (
            <Select value={chosen} onValueChange={(v) => { setPicked(v); const t = templates.find((x) => x.id === v); if (t) setTyped(JSON.stringify(t.event, null, 2)); }}>
              <SelectTrigger id={id} className="w-full max-w-[22rem] bg-card" data-testid="invoke-template"><SelectValue placeholder="Choose a template" /></SelectTrigger>
              <SelectContent>{templates.map((t) => <SelectItem key={t.id} value={t.id}>{t.label}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <JsonEditor label="Event JSON" value={event} onChange={setTyped} rows={Math.min(18, Math.max(6, event.split("\n").length + 1))} testId="invoke-event" help={big ? undefined : <>A synchronous payload can be up to 6 MB. An empty event is <span className="font-mono">{"{}"}</span>. The event is not saved; it lasts for this session only. The console sends up to 1 MiB.</>} />
        {big && <p className="text-[12.5px] text-bad">The event is over the 1 MiB the console sends.</p>}
        <div className="flex justify-end">
          <Button type="submit" disabled={!check.ok || big || invoke.isPending} data-testid="invoke-run">
            <PlayIcon /> {invoke.isPending ? "Running…" : "Test"}
          </Button>
        </div>
      </form>
      <div className="mt-4"><Refused error={refused ? null : invoke.error} operation="Invoke" testId="write-error" /></div>
      {refused && !invoke.isPending && <Throttled error={refused} fn={f} />}
      {invoke.data && !invoke.isPending && <Result r={invoke.data} />}
      {!invoke.data && !refused && (
        <div className="mt-6">
          <CallPanel request={{ service: "lambda", operation: "Invoke", input: { FunctionName: d.name, InvocationType: "RequestResponse", LogType: tail ? "Tail" : "None", Payload: "<the event JSON>" } }} showInput note="LogType Tail returns the last 4 KB of the execution log with the response." testId="invoke-call" />
        </div>
      )}
    </Card>
  );
}
