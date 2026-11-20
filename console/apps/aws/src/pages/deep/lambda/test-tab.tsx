// A function's Test tab: an event typed as JSON, invoked synchronously as the console (Live shows the Invoke), and what came
// back: whether the function answered or threw, its answer, the tail of its log, and the fields of its REPORT line.
import { PlayIcon } from "lucide-react";
import { useState } from "react";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { useInvoke } from "../../../api/deep";
import type { Invoked } from "../../../api/types/Invoked";
import { CodeBlock, ReadWith, Refused, Section, pretty } from "../parts";
import { reportOf } from "./report";

/** The most a page sends: the console takes a request body of at most 1 MiB (Lambda itself takes 6 MB). */
const MOST = 1 << 20;

function Result({ r }: { r: Invoked }) {
  const report = r.log ? reportOf(r.log) : [];
  const threw = r.function_error !== null;
  return (
    <div className="mt-8 grid gap-6" data-testid="invoke-result">
      <div>
        <h3 className="text-[15px] font-semibold text-bright">Execution result</h3>
        <div className="mt-2 flex flex-wrap items-center gap-3 text-[13.5px]">
          <StatusDot tone={threw ? "bad" : "ok"} data-testid="invoke-status">
            {threw ? `Function error: ${r.function_error}` : "Succeeded"}
          </StatusDot>
          <span className="text-muted-foreground">
            HTTP {r.status}
            {r.executed_version && <> · ran <span className="font-mono">{r.executed_version}</span></>}
          </span>
        </div>
        {threw && (
          <p className="mt-2 max-w-[760px] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            The call itself succeeded: the status is still {r.status}, and Lambda's <span className="font-mono">FunctionError</span> says {r.function_error}. The response below is the error the function threw, not a result.
          </p>
        )}
      </div>
      <div>
        <h3 className="mb-2 text-[15px] font-semibold text-bright">Response</h3>
        {r.payload ? <CodeBlock text={pretty(r.payload)} testId="invoke-payload" max="max-h-80" /> : <p className="text-[13.5px] text-muted-foreground">The function answered with nothing.</p>}
      </div>
      {r.log !== null && (
        <div>
          <h3 className="mb-2 text-[15px] font-semibold text-bright">Function logs</h3>
          <CodeBlock text={r.log} testId="invoke-log" max="max-h-80" />
          <p className="mt-2 text-[12.5px] text-muted-foreground">The last 4 KB of the run's log, as Lambda returns it with LogType Tail.</p>
        </div>
      )}
      {report.length > 0 && (
        <div>
          <h3 className="text-[15px] font-semibold text-bright">Details</h3>
          <p className="mt-1 text-[13px] text-muted-foreground">The fields of the REPORT line.</p>
          <dl className="mt-3 grid max-w-[640px] grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="invoke-report">
            {report.map(([k, v]) => (
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

export function FunctionTest({ name }: { name: string }) {
  const [event, setEvent] = useState('{\n  "key": "value"\n}');
  const [tail, setTail] = useState(true);
  const invoke = useInvoke();
  const check = checkJson(event);
  const big = new TextEncoder().encode(event).length > MOST;
  const run = (e: React.FormEvent) => {
    e.preventDefault();
    if (!check.ok || big || invoke.isPending) return;
    invoke.mutate({ FunctionName: name, Payload: event.trim() === "" ? "" : event, LogType: tail ? "Tail" : "None" });
  };
  return (
    <Section title="Test event" lede="Lambda runs the function synchronously with this event and shows what it answered. The event is not saved: it lasts as long as this page." testId="lambda-test">
      <form onSubmit={run} className="grid gap-5">
        <JsonEditor label="Event JSON" value={event} onChange={setEvent} rows={10} testId="invoke-event" help={big ? undefined : "An empty event is sent as no payload. The console sends up to 1 MiB; Lambda itself takes up to 6 MB."} />
        {big && <p className="text-[12.5px] text-bad">The event is over the 1 MiB the console sends.</p>}
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <Switch id="invoke-tail" checked={tail} onCheckedChange={setTail} data-testid="invoke-tail" />
            <Label htmlFor="invoke-tail" className="font-normal">
              Show the log tail <span className="text-muted-foreground">(LogType Tail)</span>
            </Label>
          </div>
          <Button type="submit" disabled={!check.ok || big || invoke.isPending} data-testid="invoke-run">
            <PlayIcon /> {invoke.isPending ? "Running…" : "Test"}
          </Button>
        </div>
      </form>
      <ReadWith ops={["Invoke"]} more={<>InvocationType <span className="font-mono">RequestResponse</span>.</>} />
      <div className="mt-4">
        <Refused error={invoke.error} op="Invoke" />
      </div>
      {invoke.data && !invoke.isPending && <Result r={invoke.data} />}
    </Section>
  );
}
