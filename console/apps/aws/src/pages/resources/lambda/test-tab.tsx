// A function's Test tab (lambda__function-test, test-result-ok, test-result-error, test-result-throttled): an event typed as JSON
// or started from a template shaped like the function's own triggers, invoked synchronously as the console (Live shows the
// Invoke), and what came back: the answer or the error the function threw, the tail of its log, and the fields of its REPORT
// line; or Lambda's refusal (throttled), with its details.
import { PlayIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Callout } from "@doze/ui/components/app/callout";
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

/** What an Invoke that failed says: its code and words, and the status the console passed on (a throttled call). */
function Throttled({ error, fn }: { error: ApiFailure; fn: Fn }) {
  const zero = fn.reserved === 0;
  const throttled = error.code === "TooManyRequestsException";
  return (
    <section className="mt-10 grid gap-5" data-testid="invoke-refused" aria-label="Execution result">
      <h3 className="text-[17px] font-semibold text-bright">Execution result</h3>
      <Callout tone="bad" lead={<>Invoke returned <span className="font-mono">{error.code}</span>.</>} testId="invoke-refused-words">
        {throttled ? "The request throughput limit was exceeded. The function did not run." : error.message}
      </Callout>
      <Card title="Error details" testId="invoke-refused-card">
        <dl className="grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="invoke-refused-details">
          {[["Error", error.code], ["HTTP status", String(error.status)], ["Message", error.message]].map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="border-b py-3 text-[13.5px] text-muted-foreground">{k}</dt>
              <dd className="border-b py-3 font-mono text-[13px] break-words text-bright">{v}</dd>
            </div>
          ))}
        </dl>
        {throttled && (
          <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="invoke-refused-teach">
            A throttled call never reaches your code. Throttled requests do not count as Invocations or Errors.{" "}
            <span className="text-muted-foreground">{zero ? "Setting a function's reserved concurrency to 0 throttles it on purpose; here that is why the call was refused." : `${fn.name} has ${fn.reserved === null ? "no reserved concurrency set, so the limit that was reached is the account's." : `a reserved concurrency of ${fn.reserved}, which every run in progress has used.`}`}</span>
          </p>
        )}
      </Card>
    </section>
  );
}

function Result({ r }: { r: Invoked }) {
  const report = r.log ? reportOf(r.log) : [];
  const threw = r.function_error !== null;
  const rows = threw ? ([report[0], ["FunctionError", r.function_error ?? ""], ...report.slice(1)].filter(Boolean) as [string, string][]) : report;
  return (
    <section className="mt-10 grid gap-5" data-testid="invoke-result" aria-label="Execution result">
      <h3 className="text-[17px] font-semibold text-bright">Execution result</h3>
      {threw ? (
        <Callout tone="bad" lead="The function returned an error." testId="invoke-status">The response is an error object instead of a result.</Callout>
      ) : (
        <Callout tone="good" testId="invoke-status"><b className="font-semibold text-bright">Status: Succeeded</b>{r.executed_version ? <span className="text-muted-foreground"> · ran <span className="font-mono">{r.executed_version}</span></span> : null}</Callout>
      )}
      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="grid min-w-0 gap-5">
          <Card title="Response" testId="invoke-response-card">
            {r.payload ? <CodeBlock text={pretty(r.payload)} testId="invoke-payload" max="max-h-80" /> : <p className="text-[13.5px] text-muted-foreground">The function answered with nothing.</p>}
            {threw && (
              <p className="mt-4 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="invoke-error-teach">
                The call itself succeeded: the HTTP status is still {r.status}, and the <span className="font-mono">FunctionError</span> header (<span className="font-mono">X-Amz-Function-Error</span>) says {r.function_error}.
                {r.payload.includes("/code/") && <span className="text-muted-foreground"> The file path in the trace is where doze-aws unpacked the code on this machine.</span>}
              </p>
            )}
          </Card>
          {r.log !== null && (
            <Card title="Function Logs" testId="invoke-log-card">
              <CodeBlock text={r.log} testId="invoke-log" max="max-h-80" />
              <p className="mt-3 max-w-[46rem] text-[12.5px] text-muted-foreground">The last 4 KB of the log come back with the response. The REPORT line is the same line CloudWatch Logs keeps. Init Duration appears only on a cold start. Memory Size is the configured value; the function has no memory limit locally.</p>
            </Card>
          )}
        </div>
        {rows.length > 0 && (
          <Card title="Details" lede="The Request ID and the fields of the REPORT line." testId="invoke-details-card">
            <dl className="grid grid-cols-[minmax(6rem,8rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="invoke-report">
              {rows.map(([k, v]) => (
                <div key={k} className="contents">
                  <dt className="border-b py-3 text-[13.5px] text-muted-foreground">{k}</dt>
                  <dd className={`border-b py-3 font-mono text-[13px] break-all ${k === "FunctionError" ? "font-semibold text-bad" : "text-bright"}`}>{v}</dd>
                </div>
              ))}
            </dl>
          </Card>
        )}
      </div>
    </section>
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
    <>
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
      {!invoke.data && !refused && (
        <div className="mt-6">
          <CallPanel request={{ service: "lambda", operation: "Invoke", input: { FunctionName: d.name, InvocationType: "RequestResponse", LogType: tail ? "Tail" : "None", Payload: "<the event JSON>" } }} showInput note="LogType Tail returns the last 4 KB of the execution log with the response." testId="invoke-call" />
        </div>
      )}
    </Card>
    {refused && !invoke.isPending && <Throttled error={refused} fn={f} />}
    {invoke.data && !invoke.isPending && <Result r={invoke.data} />}
    </>
  );
}
